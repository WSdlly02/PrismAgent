use crate::actors::llm_actor::model::{
    LLM_ACTOR, LlmActor, LlmHandle, LlmInferRequest, LlmInferResponse, LlmMsg, LlmStreamEvent,
};
use crate::actors::storage_actor::model::unit::Unit;
use crate::error::{SubsystemError, SubsystemResult};
use crate::impl_handle_methods;
use futures_util::StreamExt;
use genai::ServiceTarget;
use genai::adapter::AdapterKind;
use genai::chat::{ChatMessage, ChatOptions, ChatRequest, ChatStreamEvent};
use genai::resolver::{AuthData, AuthResolver, Endpoint};
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
                    let client = self.client_for(&request.model.provider, &request.model.api_key);
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

                // Mimo token plan 特殊支持
                let is_mimo = provider == "mimo";
                let mimo_use_token_plan =
                    std::env::var("MIMO_USE_TOKEN_PLAN").unwrap_or_default() == "1";

                // Sensenova 特殊支持
                let is_sensenova = provider == "sensenova";

                // 只有名字无法被 genai 自动识别的 provider 才需要绑定适配器
                // sensenova 模型名不在任何已知前缀里，会 fallback 到 Ollama，需要显式绑定
                // mimo-* 前缀能被自动识别为 AdapterKind::Mimo，不需要绑定
                let mut builder = genai::Client::builder();
                if is_sensenova {
                    builder = builder.with_adapter_kind(AdapterKind::OpenAI);
                }

                builder
                    .with_auth_resolver(AuthResolver::from_resolver_fn(move |_| {
                        Ok(Some(AuthData::from_single(api_key.clone())))
                    }))
                    .with_service_target_resolver_fn(move |mut target: ServiceTarget| {
                        if is_mimo && mimo_use_token_plan {
                            target.endpoint =
                                Endpoint::from_static("https://token-plan-cn.xiaomimimo.com/v1/");
                        }
                        if is_sensenova {
                            target.endpoint =
                                Endpoint::from_static("https://token.sensenova.cn/v1/");
                        }
                        Ok(target)
                    })
                    .with_chat_options(options)
                    .build()
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
    let stream_response = client
        .exec_chat_stream(&request.model.model_name, chat_request, None)
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

    Err(SubsystemError::Llm {
        message: "stream ended without terminal event".to_string(),
    })
}

fn llm_error(error: genai::Error) -> SubsystemError {
    SubsystemError::Llm {
        message: error.to_string(),
    }
}
