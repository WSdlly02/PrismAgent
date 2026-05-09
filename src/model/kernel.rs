use crate::model::asyncioinstance::AsyncIoHandle;
use std::collections::HashMap;
pub struct Kernel {
    pub handles: HashMap<String, AsyncIoHandle>,
}
