use crate::store::workspacestore::{WorkSpace, atomic_write_file};
use anyhow::{Result, anyhow};
use sha2::{Digest, Sha256};

impl WorkSpace {
    pub fn read_atom_store(&self, hash: &str) -> Result<Vec<u8>> {
        // $PWD/.prismagent/atoms/{hash0..2}/{hash2..}
        if hash.len() != 64 {
            return Err(anyhow!("Invalid atom hash: expected 64 hex characters"));
        }
        let atom_store_path = &self.root.join("atoms").join(&hash[0..2]).join(&hash[2..]);
        std::fs::read(&atom_store_path).map_err(|e| anyhow!("Failed to read atom store: {}", e))
    }
    pub fn write_atom_store(&self, data: &[u8]) -> Result<String> {
        if data.is_empty() {
            return Err(anyhow!("Cannot store empty atom"));
        }
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash_bytes: [u8; 32] = hasher.finalize().into();
        let hash_hex = hex::encode(hash_bytes);
        if hash_hex.len() != 64 {
            return Err(anyhow!("Invalid hash length: expected 64 hex characters"));
        }
        // $PWD/.prismagent/atoms/{hash0..2}/{hash2..}
        let atom_store_path = &self
            .root
            .join("atoms")
            .join(&hash_hex[0..2])
            .join(&hash_hex[2..]);
        if atom_store_path.exists() {
            return Ok(hash_hex);
        }
        atomic_write_file(atom_store_path, data)?;
        Ok(hash_hex)
    }
}
