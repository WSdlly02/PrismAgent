use crate::model::unit::Unit;
use anyhow::{Result, anyhow};
use std::path::PathBuf;

// const UNIT_STORE_DIR: &str = ".prismagent/runs/{run-id}/units";
// 单元路径是传入的函数参数

pub fn read_unit_store(unit_src: &PathBuf) -> Result<Unit> {
    std::fs::read(unit_src)
        .map_err(|e| anyhow!("Failed to read unit store: {}", e))
        .and_then(|data| {
            serde_json::from_slice(&data)
                .map_err(|e| anyhow!("Failed to parse unit store JSON: {}", e))
        })
}
pub fn write_unit_store(unit_dst: &PathBuf, unit: &Unit) -> Result<()> {
    std::fs::create_dir_all(
        unit_dst
            .parent()
            .ok_or_else(|| anyhow!("Invalid unit store path: no parent directory"))?,
    )?;
    let data =
        serde_json::to_vec(unit).map_err(|e| anyhow!("Failed to serialize unit to JSON: {}", e))?;
    std::fs::write(unit_dst, data).map_err(|e| anyhow!("Failed to write unit store: {}", e))
}
