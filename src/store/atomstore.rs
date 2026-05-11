use crate::model::atom::Atom;
use crate::model::workspace::WorkSpace;
use crate::store::workspacestore::atomic_create_file;
use anyhow::{Result, anyhow};
use sha2::{Digest, Sha256};

impl WorkSpace {
    pub fn read_atom_store(&self, hash: &str) -> Result<Atom> {
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
        atomic_create_file(atom_store_path, data)?;
        Ok(hash_hex)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::workspace::{
        ConcurrencyConfigSection, EntryMode, LockScope, RuntimeConfigSection, WorkspaceConfig,
        WorkspaceConfigSection,
    };
    use std::path::PathBuf;
    use uuid::Uuid;

    fn test_workspace() -> WorkSpace {
        let root = std::env::temp_dir()
            .join("prismagent-tests")
            .join(Uuid::new_v4().to_string())
            .join(".prismagent");
        WorkSpace {
            root: PathBuf::from(root),
            workspace_config: WorkspaceConfig {
                workspace: WorkspaceConfigSection { state_version: 1 },
                runtime: RuntimeConfigSection {
                    entry_mode: EntryMode::ManualResume,
                },
                concurrency: ConcurrencyConfigSection {
                    lock_scope: LockScope::Run,
                },
            },
        }
    }

    #[test]
    fn atom_store_writes_by_sha256_and_reads_back() {
        let workspace = test_workspace();
        let hash = workspace
            .write_atom_store(b"hello atom")
            .expect("write atom");

        assert_eq!(hash.len(), 64);
        assert_eq!(
            workspace.read_atom_store(&hash).expect("read atom"),
            b"hello atom"
        );
        assert!(
            workspace
                .root
                .join("atoms")
                .join(&hash[..2])
                .join(&hash[2..])
                .is_file()
        );
    }

    #[test]
    fn atom_store_is_idempotent_for_same_content() {
        let workspace = test_workspace();
        let first = workspace.write_atom_store(b"same").expect("first write");
        let second = workspace.write_atom_store(b"same").expect("second write");

        assert_eq!(first, second);
    }

    #[test]
    fn atom_store_rejects_empty_or_invalid_hash() {
        let workspace = test_workspace();

        assert!(workspace.write_atom_store(b"").is_err());
        assert!(workspace.read_atom_store("not-a-valid-hash").is_err());
    }
}
