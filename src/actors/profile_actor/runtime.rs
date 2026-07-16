use crate::actor_dispatch;
use crate::actors::profile_actor::model::{
    FinalModelConfig, PROFILE_ACTOR, Profile, ProfileActor, ProfileHandle, ProfileMsg,
    PromptsConfigSection, ToolsConfigSection,
};
use crate::error::{ResourceKind, SubsystemError, SubsystemResult};
use crate::impl_handle_methods;
use crate::stdlib_assets;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

impl ProfileActor {
    pub fn load(rx: mpsc::Receiver<ProfileMsg>) -> SubsystemResult<Self> {
        let root = default_profiles_root()?;
        Self::from_root(rx, root)
    }

    pub fn from_root(rx: mpsc::Receiver<ProfileMsg>, root: PathBuf) -> SubsystemResult<Self> {
        std::fs::create_dir_all(&root).map_err(|error| {
            SubsystemError::io("create profile directory", Some(root.clone()), error)
        })?;
        // 将缺失的嵌入 profile 写出到文件系统
        stdlib_assets::bootstrap_profiles(&root)
            .map_err(|error| SubsystemError::io("bootstrap profiles", Some(root.clone()), error))?;
        let profiles = load_profiles(&root)?;
        Ok(Self { rx, root, profiles })
    }

    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.run())
    }

    pub async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            actor_dispatch!(msg;
                ProfileMsg::ListProfiles { ; reply } => self.list_profiles(),
                ProfileMsg::GetProfile { name ; reply } => self.profile(&name).cloned(),
                ProfileMsg::GetModelConfig { profile_name ; reply } => self.model_config(&profile_name),
                ProfileMsg::GetPrompts { profile_name ; reply } => self.profile(&profile_name).map(|p| p.prompts.clone()),
                ProfileMsg::GetTools { profile_name ; reply } => self.profile(&profile_name).map(|p| p.tools.clone())
            );
        }
    }

    fn list_profiles(&mut self) -> SubsystemResult<Vec<String>> {
        for (name, profile) in load_profiles(&self.root)? {
            self.profiles.entry(name).or_insert(profile);
        }
        let mut names = self.profiles.keys().cloned().collect::<Vec<_>>();
        names.sort();
        Ok(names)
    }

    fn profile(&self, name: &str) -> SubsystemResult<&Profile> {
        self.profiles
            .get(name)
            .ok_or_else(|| SubsystemError::not_found(ResourceKind::Profile, name))
    }

    fn model_config(&self, profile_name: &str) -> SubsystemResult<FinalModelConfig> {
        let model = self.profile(profile_name)?.model.clone();
        let api_key = std::env::var(&model.api_key_env).map_err(|_| {
            SubsystemError::configuration(
                "llm credentials",
                format!("environment variable {} is not set", model.api_key_env),
            )
        })?;
        Ok(FinalModelConfig {
            provider: model.provider,
            model_name: model.model_name,
            api_key,
        })
    }
}

// ---- Declarative macro: handle method with no params ----

impl_handle_methods! {
    ProfileHandle for ProfileMsg, PROFILE_ACTOR;

    fn list_profiles(&self) -> Vec<String>
        => ListProfiles {};

    fn profile(&self, name: impl Into<String>) -> Profile
        => GetProfile { name: name.into() };

    fn model_config(&self, profile_name: impl Into<String>) -> FinalModelConfig
        => GetModelConfig { profile_name: profile_name.into() };

    fn prompts(&self, profile_name: impl Into<String>) -> PromptsConfigSection
        => GetPrompts { profile_name: profile_name.into() };

    fn tools(&self, profile_name: impl Into<String>) -> ToolsConfigSection
        => GetTools { profile_name: profile_name.into() };
}

fn default_profiles_root() -> SubsystemResult<PathBuf> {
    Ok(dirs::home_dir()
        .ok_or_else(|| {
            SubsystemError::internal("resolve profile directory", "home directory is unavailable")
        })?
        .join(".prismagent")
        .join("profiles"))
}

/// 从文件系统加载所有 profile。
/// bootstrap_profiles() 已保证目录中存在所有嵌入的 profile，
/// 因此这里只管读磁盘，不需要 fallback 逻辑。
/// 用户可自由编辑或删除文件系统的 profile，
/// 下次启动时被删除的嵌入 profile 会重新写出。
fn load_profiles(root: &Path) -> SubsystemResult<HashMap<String, Profile>> {
    let mut profiles = HashMap::new();
    if root.exists() {
        let entries = std::fs::read_dir(root).map_err(|error| {
            SubsystemError::io("list profile directory", Some(root.to_path_buf()), error)
        })?;
        for entry in entries {
            let path = entry
                .map_err(|error| {
                    SubsystemError::io(
                        "read profile directory entry",
                        Some(root.to_path_buf()),
                        error,
                    )
                })?
                .path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                continue;
            }
            let profile = read_profile(&path)?;
            profiles.insert(profile.name.clone(), profile);
        }
    }
    Ok(profiles)
}

fn read_profile(path: &Path) -> SubsystemResult<Profile> {
    let data = std::fs::read_to_string(path)
        .map_err(|error| SubsystemError::io("read profile", Some(path.to_path_buf()), error))?;
    let profile: Profile = toml::from_str(&data).map_err(|error| {
        SubsystemError::corrupt_state("profile file", format!("{}: {error}", path.display()))
    })?;
    let expected_name = path.file_stem().and_then(|stem| stem.to_str());
    if expected_name != Some(profile.name.as_str()) {
        return Err(SubsystemError::corrupt_state(
            "profile file",
            format!(
                "{}: profile name {} does not match file name {}",
                path.display(),
                profile.name,
                expected_name.unwrap_or("<invalid>")
            ),
        ));
    }
    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn loads_all_embedded_profiles_when_directory_is_empty() {
        let root = std::env::temp_dir().join(format!("prismagent-profiles-{}", Uuid::now_v7()));
        let (tx, rx) = mpsc::channel::<ProfileMsg>(8);
        ProfileActor::from_root(rx, root).unwrap().spawn();
        let handle = ProfileHandle { tx };

        let names = handle.list_profiles().await.unwrap();
        // 应该包含所有的嵌入 profile
        for embedded in stdlib_assets::EMBEDDED_PROFILES {
            assert!(
                names.contains(&embedded.name.to_string()),
                "missing: {}",
                embedded.name
            );
        }
    }

    #[tokio::test]
    async fn filesystem_profile_overrides_embedded() {
        let root = std::env::temp_dir().join(format!("prismagent-profiles-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&root).unwrap();
        // 写一个自定义的 default.toml
        let custom = r#"name = "default"

[model]
provider = "deepseek"
model_name = "custom-model"
api_key_env = "CUSTOM_KEY"

[prompts]
system.identity = "custom"
system.behavior = "custom"
system.response_style = "custom"
system.capabilities = "{skills} {tools}"
auto_loop = false
auto_loop_message = ""

[tools]
yolo = false
available_tools = ["fs_tree_list"]
auto_approve = ["fs_tree_list"]
"#;
        std::fs::write(root.join("default.toml"), custom).unwrap();

        let (tx, rx) = mpsc::channel::<ProfileMsg>(8);
        ProfileActor::from_root(rx, root).unwrap().spawn();
        let handle = ProfileHandle { tx };

        let profile = handle.profile("default").await.unwrap();
        assert_eq!(profile.model.model_name, "custom-model");
        assert_eq!(profile.model.api_key_env, "CUSTOM_KEY");

        let names = handle.list_profiles().await.unwrap();
        assert!(names.contains(&"planner".to_string()));
    }
}
