use anyhow::{Result, anyhow};
use sha2::{Digest, Sha256};
use std::fs::create_dir_all;
use std::path::PathBuf;

const ATOM_STORE_DIR: &str = ".prismagent/atoms";

pub fn read_atom_store(current_dir: &PathBuf, hash: &str) -> Result<Vec<u8>> {
    // $PWD/.prismagent/atoms/{hash0..2}/{hash2..}
    let atom_store_path = current_dir
        .join(ATOM_STORE_DIR)
        .join(&hash[0..2])
        .join(&hash[2..]);
    std::fs::read(&atom_store_path).map_err(|e| anyhow!("Failed to read atom store: {}", e))
}
pub fn write_atom_store(current_dir: &PathBuf, data: &[u8]) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hash: [u8; 32] = hasher.finalize().into();
    let hash_hex = hex::encode(hash);

    // $PWD/.prismagent/atoms/{hash0..2}/{hash2..}
    let atom_store_path = current_dir
        .join(ATOM_STORE_DIR)
        .join(&hash_hex[0..2])
        .join(&hash_hex[2..]);
    if atom_store_path.exists() {
        return Ok(hash_hex); // 已经存在相同内容的Atom，无需重复写入
    }
    // 创建目录: $PWD/.prismagent/atoms/{hash0..2}
    create_dir_all(current_dir.join(ATOM_STORE_DIR).join(&hash_hex[0..2]))?;
    std::fs::write(&atom_store_path, data)
        .map_err(|e| anyhow!("Failed to write atom store: {}", e))?;
    Ok(hash_hex)
}
