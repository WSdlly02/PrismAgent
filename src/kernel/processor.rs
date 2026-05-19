use crate::model::unit::{Unit, UnitRole};
use anyhow::{Result, anyhow};
use genai::chat::ChatMessage;
use std::path::PathBuf;

/// 将 Unit 转换为 ChatMessage，供 LLM 实例调用时使用。
/// 只转换 System/User/Assistant/Tool 四种角色，其他角色被过滤掉。
/// 例如在工具调用请求与工具调用
fn convert_units_to_chat_request(run_root: &PathBuf, units: &[Unit]) -> Result<Vec<ChatMessage>> {
    let contents = read_units_correspond_atoms(run_root, units)?;
    Ok(units
        .iter()
        .zip(contents.into_iter())
        .map(|(unit, content)| match unit.role {
            UnitRole::System => ChatMessage::system(content),
            UnitRole::User => ChatMessage::user(content),
            UnitRole::Assistant => ChatMessage::assistant(content),
            UnitRole::Tool => ChatMessage::tool(content),
        })
        .collect())
}

fn read_units_correspond_atoms(run_root: &PathBuf, units: &[Unit]) -> Result<Vec<String>> {
    // run_root -> .prismagent/runs/{uuid}
    // atom 存储在 .prismagent/atoms/{atom_hash:2}/{atom_hash2..}
    units
        .iter()
        .map(|unit| {
            std::fs::read_to_string(
                run_root
                    .parent() // .prismagent/runs
                    .unwrap()
                    .parent() // .prismagent
                    .unwrap()
                    .join("atoms")
                    .join(&unit.atom_hash[0..2])
                    .join(&unit.atom_hash[2..]),
            )
            .map_err(|e| anyhow!("Failed to read atom file: {}", e))
        })
        .collect()
}
