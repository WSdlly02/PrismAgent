use anyhow::{Result, anyhow};
use std::path::PathBuf;
pub struct WorkSpace {
    pub root: PathBuf, // $PWD/.prismagent
}

impl WorkSpace {
    pub fn resume_or_init_workspace() -> Result<Self> {
        let root = std::env::current_dir()?.join(".prismagent");
        if root.is_dir() {
            return Ok(Self { root });
        }
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }
    pub fn resume_or_init_workspace_metadata(&self) -> Result<()> {
        Ok(())
    }
    pub fn get_root(&self) -> &PathBuf {
        &self.root
    }
}

pub(crate) fn atomic_write_file(dst: &PathBuf, data: &[u8]) -> Result<()> {
    if dst.exists() {
        return Err(anyhow!("File already exists: {}", dst.display()));
    }
    std::fs::create_dir_all(
        dst.parent()
            .ok_or_else(|| anyhow!("Invalid path: no parent directory"))?,
    )?;
    let tmp_dst = dst.with_extension("tmp");
    std::fs::write(&tmp_dst, data)?;
    std::fs::rename(tmp_dst, dst)?;
    Ok(())
}
