use crate::actors::context_actor::model::{
    BuildMessagesRequest, CONTEXT_ACTOR, ContextActor, ContextHandle, ContextMsg,
    ContextRenderRequest, ContextResolveRequest, ContextResolveResponse,
};
use crate::error::{SubsystemError, SubsystemResult};
use crate::handles::AppHandles;
use genai::chat::ChatMessage;
use tokio::sync::mpsc;

impl ContextActor {
    pub fn load(rx: mpsc::Receiver<ContextMsg>, handles: AppHandles) -> Self {
        Self { rx, handles }
    }

    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.run())
    }

    pub async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                ContextMsg::ListContexts { reply } => {
                    let _ = reply.send(self.handles.storage.list_contexts().await);
                }
                ContextMsg::ReadContexts { uuids, reply } => {
                    let _ = reply.send(self.handles.storage.read_contexts(uuids).await);
                }
                ContextMsg::WriteContexts { contexts, reply } => {
                    let _ = reply.send(self.handles.storage.write_contexts(contexts).await);
                }
                ContextMsg::Resolve { request, reply } => {
                    let _ = reply.send(self.resolve(request).await);
                }
                ContextMsg::Render { request, reply } => {
                    let _ = reply.send(self.render(request).await);
                }
                ContextMsg::BuildMessages { request, reply } => {
                    let _ = reply.send(self.build_messages(request).await);
                }
            }
        }
    }

    async fn resolve(
        &self,
        request: ContextResolveRequest,
    ) -> SubsystemResult<ContextResolveResponse> {
        let units = if request.unit_uuids.is_empty() {
            Vec::new()
        } else {
            self.handles.storage.read_units(request.unit_uuids).await?
        };

        let contexts = if request.context_uuids.is_empty() {
            Vec::new()
        } else {
            self.handles
                .storage
                .read_contexts(request.context_uuids)
                .await?
        };

        Ok(ContextResolveResponse { units, contexts })
    }

    async fn render(&self, request: ContextRenderRequest) -> SubsystemResult<String> {
        let resolved = self
            .resolve(ContextResolveRequest {
                unit_uuids: request.unit_uuids,
                context_uuids: request.context_uuids,
            })
            .await?;
        Ok(render_context_inputs(&resolved))
    }

    async fn build_messages(
        &self,
        request: BuildMessagesRequest,
    ) -> SubsystemResult<Vec<ChatMessage>> {
        let resolved = self
            .resolve(ContextResolveRequest {
                unit_uuids: request.unit_uuids,
                context_uuids: request.context_uuids,
            })
            .await?;

        let mut messages = Vec::new();
        let rendered_context = render_context_inputs(&resolved);
        if !rendered_context.trim().is_empty() {
            messages.push(ChatMessage::system(rendered_context));
        }
        messages.extend(resolved.units.iter().map(|unit| unit.to_chat_message()));
        if let Some(user_input) = request.user_input
            && !user_input.trim().is_empty()
        {
            messages.push(ChatMessage::user(user_input));
        }
        Ok(messages)
    }
}

impl ContextHandle {
    pub async fn list_contexts(&self) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(ContextMsg::ListContexts { reply: reply_tx })
            .await
            .map_err(|_| SubsystemError::actor_dead(CONTEXT_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(CONTEXT_ACTOR))?
    }

    pub async fn read_contexts(
        &self,
        uuids: Vec<String>,
    ) -> SubsystemResult<Vec<crate::actors::storage_actor::model::context::Context>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(ContextMsg::ReadContexts {
                uuids,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(CONTEXT_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(CONTEXT_ACTOR))?
    }

    pub async fn write_contexts(
        &self,
        contexts: Vec<crate::actors::storage_actor::model::context::Context>,
    ) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(ContextMsg::WriteContexts {
                contexts,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(CONTEXT_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(CONTEXT_ACTOR))?
    }

    pub async fn resolve(
        &self,
        request: ContextResolveRequest,
    ) -> SubsystemResult<ContextResolveResponse> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(ContextMsg::Resolve {
                request,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(CONTEXT_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(CONTEXT_ACTOR))?
    }

    pub async fn render(&self, request: ContextRenderRequest) -> SubsystemResult<String> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(ContextMsg::Render {
                request,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(CONTEXT_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(CONTEXT_ACTOR))?
    }

    pub async fn build_messages(
        &self,
        request: BuildMessagesRequest,
    ) -> SubsystemResult<Vec<ChatMessage>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(ContextMsg::BuildMessages {
                request,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(CONTEXT_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(CONTEXT_ACTOR))?
    }
}

fn render_context_inputs(resolved: &ContextResolveResponse) -> String {
    let mut sections = Vec::new();

    if !resolved.contexts.is_empty() {
        let rendered_contexts = resolved
            .contexts
            .iter()
            .map(|context| {
                format!(
                    "## Context: {}\n\n{}\n",
                    context.title.trim(),
                    context.content.trim()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("# Context Documents\n\n{rendered_contexts}"));
    }

    if !resolved.units.is_empty() {
        let rendered_units = resolved
            .units
            .iter()
            .map(|unit| {
                let content = serde_json::to_string_pretty(&unit.content)
                    .unwrap_or_else(|error| format!("{{\"error\":\"{error}\"}}"));
                format!("## Unit: {}\n\n```json\n{}\n```\n", unit.uuid, content)
            })
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("# Referenced Units\n\n{rendered_units}"));
    }

    sections.join("\n\n")
}
