use crate::actors::config_actor::model::ConfigHandle;
use crate::actors::context_actor::model::ContextHandle;
use crate::actors::storage_actor::model::StorageHandle;

#[derive(Clone)]
pub struct AppHandles {
    pub config: ConfigHandle,
    pub context: ContextHandle,
    pub storage: StorageHandle,
}
