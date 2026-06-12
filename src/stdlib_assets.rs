use std::path::Path;

/// 编译时嵌入 stdlib/profiles 和 stdlib/skills 目录下的文件。
/// 启动时，缺失的嵌入文件会被写出到文件系统，保证磁盘上始终有完整的基础配置。
// ---------------------------------------------------------------------------
// 嵌入的数据
// ---------------------------------------------------------------------------
pub struct EmbeddedProfile {
    pub name: &'static str,
    pub filename: &'static str,
    pub content: &'static str,
}

pub struct EmbeddedSkillFile {
    /// 相对于 skill 目录的路径，如 "SKILL.md" 或 "prompts/review.md"
    pub relative_path: &'static str,
    pub content: &'static str,
}

pub struct EmbeddedSkill {
    /// skill 目录名，如 "multi-agent-collaboration"
    pub dirname: &'static str,
    /// 该 skill 目录下的所有文件
    pub files: &'static [EmbeddedSkillFile],
}

pub const DEFAULT_PROFILE_NAME: &str = "default";

pub const EMBEDDED_PROFILES: &[EmbeddedProfile] = &[
    EmbeddedProfile {
        name: "default",
        filename: "default.toml",
        content: include_str!("../stdlib/profiles/default.toml"),
    },
    EmbeddedProfile {
        name: "planner",
        filename: "planner.toml",
        content: include_str!("../stdlib/profiles/planner.toml"),
    },
    EmbeddedProfile {
        name: "executor",
        filename: "executor.toml",
        content: include_str!("../stdlib/profiles/executor.toml"),
    },
    EmbeddedProfile {
        name: "verifier",
        filename: "verifier.toml",
        content: include_str!("../stdlib/profiles/verifier.toml"),
    },
];

pub const EMBEDDED_SKILLS: &[EmbeddedSkill] = &[EmbeddedSkill {
    dirname: "multi-agent-collaboration",
    files: &[
        EmbeddedSkillFile {
            relative_path: "SKILL.md",
            content: include_str!("../stdlib/skills/multi-agent-collaboration/SKILL.md"),
        },
        EmbeddedSkillFile {
            relative_path: "assets/workflow-example.toml",
            content: include_str!(
                "../stdlib/skills/multi-agent-collaboration/assets/workflow-example.toml"
            ),
        },
        EmbeddedSkillFile {
            relative_path: "references/default.md",
            content: include_str!(
                "../stdlib/skills/multi-agent-collaboration/references/default.md"
            ),
        },
        EmbeddedSkillFile {
            relative_path: "references/planner.md",
            content: include_str!(
                "../stdlib/skills/multi-agent-collaboration/references/planner.md"
            ),
        },
        EmbeddedSkillFile {
            relative_path: "references/executor.md",
            content: include_str!(
                "../stdlib/skills/multi-agent-collaboration/references/executor.md"
            ),
        },
        EmbeddedSkillFile {
            relative_path: "references/verifier.md",
            content: include_str!(
                "../stdlib/skills/multi-agent-collaboration/references/verifier.md"
            ),
        },
    ],
}];

// ---------------------------------------------------------------------------
// Bootstrap：将文件系统上缺失的嵌入文件写出到磁盘
// ---------------------------------------------------------------------------

/// 确保 `profiles_dir`（如 `~/.prismagent/profiles/`）中有所有嵌入的 profile。
/// 已存在的文件不会被覆盖（用户修改优先）。
pub fn bootstrap_profiles(profiles_dir: &Path) -> Result<(), std::io::Error> {
    std::fs::create_dir_all(profiles_dir)?;
    for ep in EMBEDDED_PROFILES {
        let target = profiles_dir.join(ep.filename);
        if !target.exists() {
            std::fs::write(&target, ep.content)?;
        }
    }
    Ok(())
}

/// 确保 `skills_dir`（如 `~/.prismagent/skills/`）中有所有嵌入的 skill 目录及文件。
/// 已存在的文件不会被覆盖（用户修改优先）。
/// 子目录结构由 `relative_path` 自动创建。
pub fn bootstrap_skills(skills_dir: &Path) -> Result<(), std::io::Error> {
    std::fs::create_dir_all(skills_dir)?;
    for es in EMBEDDED_SKILLS {
        let skill_root = skills_dir.join(es.dirname);
        for file in es.files {
            let target = skill_root.join(file.relative_path);
            if !target.exists() {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&target, file.content)?;
            }
        }
    }
    Ok(())
}
