use crate::actors::config_actor::model::ConfigHandle;

#[derive(Clone)]
pub struct AppHandles {
    pub config: ConfigHandle,
}
