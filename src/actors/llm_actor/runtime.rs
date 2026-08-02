use crate::actors::llm_actor::adapters;
use crate::actors::llm_actor::model::{
    LLM_ACTOR, LlmActor, LlmHandle, LlmInferRequest, LlmInferResponse, LlmMsg, LlmStreamEvent,
};
use crate::actors::profile_actor::model::{
    FinalModelConfig, ReasoningEffortConfig, ReasoningEffortLevel,
};
use crate::actors::storage_actor::model::unit::Unit;
use crate::error::{ErrorClass, ExternalKind, SubsystemError, SubsystemResult};
use crate::impl_handle_methods;
use futures_util::StreamExt;
use genai::chat::{ChatMessage, ChatOptions, ChatRequest, ChatStreamEvent, ReasoningEffort};
use genai::resolver::{AuthData, AuthResolver};
use std::collections::HashMap;
use tokio::sync::mpsc;

impl LlmActor {
    pub fn load(rx: mpsc::Receiver<LlmMsg>) -> Self {
        Self {
            rx,
            clients: HashMap::new(),
            inflight: HashMap::new(),
        }
    }

    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.run())
    }

    pub async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            self.prune_finished();
            match msg {
                LlmMsg::Infer { request, reply } => {
                    let client = self.client_for(&request.config.provider, &request.config.api_key);
                    let inference_uuid = request.inference_uuid.clone();
                    let task = tokio::spawn(async move {
                        let result = run_streaming_inference(client, request).await;
                        let _ = reply.send(result);
                    });
                    self.inflight.insert(inference_uuid, task);
                }
                LlmMsg::Cancel {
                    inference_uuid,
                    reply,
                } => {
                    let cancelled = self
                        .inflight
                        .remove(&inference_uuid)
                        .map(|task| {
                            task.abort();
                            true
                        })
                        .unwrap_or(false);
                    let _ = reply.send(Ok(cancelled));
                }
            }
        }
    }

    fn client_for(&mut self, provider: &str, api_key: &str) -> genai::Client {
        self.clients
            .entry(provider.to_string())
            .or_insert_with(|| {
                let api_key = api_key.to_string();
                let options = ChatOptions::default()
                    .with_capture_content(true)
                    .with_capture_usage(true)
                    .with_capture_reasoning_content(true)
                    .with_capture_tool_calls(true);

                let builder = genai::Client::builder()
                    .with_auth_resolver(AuthResolver::from_resolver_fn(
                        // ModelIden is not used in current case
                        |_| Ok(Some(AuthData::from_single(api_key))),
                    ))
                    .with_chat_options(options);
                match provider {
                    "mimo" => adapters::mimo::build_mimo_client(builder),
                    "sensenova" => adapters::sensenova::build_sensenova_client(builder),
                    _ => builder.build(),
                }
            })
            .clone()
    }

    fn prune_finished(&mut self) {
        self.inflight.retain(|_, task| !task.is_finished());
    }
}

// ---- Declarative macro: handle method with concrete type ----

impl_handle_methods! {
    LlmHandle for LlmMsg, LLM_ACTOR;

    fn infer(&self, request: LlmInferRequest) -> LlmInferResponse
        => Infer { request: request };

    fn cancel(&self, inference_uuid: impl Into<String>) -> bool
        => Cancel { inference_uuid: inference_uuid.into() };
}

async fn run_streaming_inference(
    client: genai::Client,
    request: LlmInferRequest,
) -> SubsystemResult<LlmInferResponse> {
    let _ = request.stream_tx.send(LlmStreamEvent::Started).await;
    let messages = request
        .units
        .iter()
        .map(Unit::to_chat_message)
        .collect::<Vec<ChatMessage>>();
    let mut chat_request = ChatRequest::from_messages(messages);
    if !request.tools.is_empty() {
        chat_request = chat_request.with_tools(request.tools);
    }
    let options = request_chat_options(&request.config);
    let stream_response = client
        .exec_chat_stream(&request.config.model_name, chat_request, options.as_ref())
        .await
        .map_err(llm_error)?;
    let mut stream = stream_response.stream;

    while let Some(event) = stream.next().await {
        match event.map_err(llm_error)? {
            ChatStreamEvent::Start => {}
            ChatStreamEvent::Chunk(chunk) => {
                let _ = request
                    .stream_tx
                    .send(LlmStreamEvent::TextDelta {
                        text: chunk.content,
                    })
                    .await;
            }
            ChatStreamEvent::ReasoningChunk(chunk) => {
                let _ = request
                    .stream_tx
                    .send(LlmStreamEvent::ReasoningDelta {
                        text: chunk.content,
                    })
                    .await;
            }
            ChatStreamEvent::ThoughtSignatureChunk(_) => {}
            ChatStreamEvent::ToolCallChunk(chunk) => {
                let _ = request
                    .stream_tx
                    .send(LlmStreamEvent::ToolCallDelta {
                        name: Some(chunk.tool_call.fn_name),
                    })
                    .await;
            }
            ChatStreamEvent::End(stream_end) => {
                let usage = stream_end.captured_usage.clone();
                let has_tool_calls = stream_end
                    .captured_tool_calls()
                    .is_some_and(|tool_calls| !tool_calls.is_empty());
                let message = if has_tool_calls {
                    stream_end
                        .into_assistant_message_for_tool_use()
                        .unwrap_or_else(|| ChatMessage::assistant(""))
                } else {
                    let text = stream_end.captured_first_text().unwrap_or("").to_string();
                    ChatMessage::assistant(text)
                        .with_reasoning_content(stream_end.captured_reasoning_content)
                };
                let output_unit = Unit::from_chat_message_with_usage(message, usage);
                let _ = request.stream_tx.send(LlmStreamEvent::Finished).await;
                return Ok(LlmInferResponse {
                    output_unit,
                    is_tool_calls: has_tool_calls,
                });
            }
        }
    }

    Err(SubsystemError::external(
        ExternalKind::Llm,
        ErrorClass::Unavailable,
        "stream ended without terminal event",
        true,
    ))
}

fn request_chat_options(config: &FinalModelConfig) -> Option<ChatOptions> {
    match &config.reasoning_effort {
        Some(effort) => match effort {
            ReasoningEffortConfig::Level(level) => match level {
                ReasoningEffortLevel::None => {
                    Some(ChatOptions::default().with_reasoning_effort(ReasoningEffort::None))
                }
                ReasoningEffortLevel::Low => {
                    Some(ChatOptions::default().with_reasoning_effort(ReasoningEffort::Low))
                }
                ReasoningEffortLevel::Medium => {
                    Some(ChatOptions::default().with_reasoning_effort(ReasoningEffort::Medium))
                }
                ReasoningEffortLevel::High => {
                    Some(ChatOptions::default().with_reasoning_effort(ReasoningEffort::High))
                }
                ReasoningEffortLevel::XHigh => {
                    Some(ChatOptions::default().with_reasoning_effort(ReasoningEffort::XHigh))
                }
                ReasoningEffortLevel::Max => {
                    Some(ChatOptions::default().with_reasoning_effort(ReasoningEffort::Max))
                }
            },
            ReasoningEffortConfig::Budget { budget } => {
                Some(ChatOptions::default().with_reasoning_effort(ReasoningEffort::Budget(*budget)))
            }
        },
        None => None,
    }
}

fn llm_error(error: genai::Error) -> SubsystemError {
    let (class, retryable) = genai_error_semantics(&error);
    SubsystemError::external(ExternalKind::Llm, class, error.to_string(), retryable)
}

fn genai_error_semantics(error: &genai::Error) -> (ErrorClass, bool) {
    match error {
        genai::Error::NoChatResponse { .. }
        | genai::Error::InvalidJsonResponseElement { .. }
        | genai::Error::ChatResponseGeneration { .. }
        | genai::Error::ChatResponse { .. }
        | genai::Error::StreamParse { .. }
        | genai::Error::WebStream { .. } => (ErrorClass::Unavailable, true),
        genai::Error::HttpError { status, .. } => http_error_semantics(*status),
        genai::Error::WebAdapterCall { webc_error, .. }
        | genai::Error::WebModelCall { webc_error, .. } => webc_error_semantics(webc_error),
        _ => (ErrorClass::Internal, false),
    }
}

fn webc_error_semantics(error: &genai::webc::Error) -> (ErrorClass, bool) {
    match error {
        genai::webc::Error::ResponseFailedNotJson { .. }
        | genai::webc::Error::ResponseFailedInvalidJson { .. } => (ErrorClass::Unavailable, true),
        genai::webc::Error::ResponseFailedStatus { status, .. } => http_error_semantics(*status),
        genai::webc::Error::Reqwest(error) if error.is_timeout() => (ErrorClass::Timeout, true),
        genai::webc::Error::Reqwest(error) if error.is_connect() => (ErrorClass::Unavailable, true),
        genai::webc::Error::Reqwest(_) | genai::webc::Error::JsonValueExt(_) => {
            (ErrorClass::Internal, false)
        }
    }
}

fn http_error_semantics(status: reqwest::StatusCode) -> (ErrorClass, bool) {
    if status == reqwest::StatusCode::REQUEST_TIMEOUT
        || status == reqwest::StatusCode::GATEWAY_TIMEOUT
    {
        (ErrorClass::Timeout, true)
    } else if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
        (ErrorClass::Unavailable, true)
    } else {
        // Provider-side 4xx responses usually indicate invalid server
        // credentials, model configuration, or an adapter-built request. They
        // must not be reported as a transient service outage.
        (ErrorClass::Internal, false)
    }
}

#[cfg(test)]
mod tests {
    use super::{http_error_semantics, request_chat_options};
    use crate::actors::profile_actor::model::{
        FinalModelConfig, ReasoningEffortConfig, ReasoningEffortLevel,
    };
    use crate::error::ErrorClass;
    use genai::chat::ReasoningEffort;
    use reqwest::StatusCode;

    #[test]
    fn classifies_provider_statuses_without_conflating_rejection_and_outage() {
        assert_eq!(
            http_error_semantics(StatusCode::REQUEST_TIMEOUT),
            (ErrorClass::Timeout, true)
        );
        assert_eq!(
            http_error_semantics(StatusCode::TOO_MANY_REQUESTS),
            (ErrorClass::Unavailable, true)
        );
        assert_eq!(
            http_error_semantics(StatusCode::BAD_GATEWAY),
            (ErrorClass::Unavailable, true)
        );
        assert_eq!(
            http_error_semantics(StatusCode::BAD_REQUEST),
            (ErrorClass::Internal, false)
        );
        assert_eq!(
            http_error_semantics(StatusCode::UNAUTHORIZED),
            (ErrorClass::Internal, false)
        );
    }

    #[test]
    fn reasoning_effort_is_applied_per_request() {
        let mut model = FinalModelConfig {
            provider: "openai".to_string(),
            model_name: "gpt-5.2".to_string(),
            api_key: "test".to_string(),
            reasoning_effort: None,
        };
        assert!(request_chat_options(&model).is_none());

        model.reasoning_effort = Some(ReasoningEffortConfig::Level(ReasoningEffortLevel::High));
        let high = request_chat_options(&model).expect("request options");
        assert!(matches!(high.reasoning_effort, Some(ReasoningEffort::High)));

        model.reasoning_effort = Some(ReasoningEffortConfig::Budget { budget: 8000 });
        let budget = request_chat_options(&model).expect("request options");
        assert!(matches!(
            budget.reasoning_effort,
            Some(ReasoningEffort::Budget(8000))
        ));
        assert!(matches!(high.reasoning_effort, Some(ReasoningEffort::High)));
    }
}
