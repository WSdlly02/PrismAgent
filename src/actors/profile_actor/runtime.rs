use crate::actors::profile_actor::model::{
    DEFAULT_PROFILE, DEFAULT_PROFILE_NAME, FinalModelConfig, PROFILE_ACTOR, Profile, ProfileActor,
    ProfileHandle, ProfileMsg, PromptsConfigSection, ToolsConfigSection,
};
use crate::error::{SubsystemError, SubsystemResult};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

impl ProfileActor {
    pub fn load(rx: mpsc::Receiver<ProfileMsg>) -> SubsystemResult<Self> {
        let root = default_profiles_root()?;
        Self::from_root(rx, root)
    }

    pub fn from_root(rx: mpsc::Receiver<ProfileMsg>, root: PathBuf) -> SubsystemResult<Self> {
        std::fs::create_dir_all(&root)?;
        let profiles = load_profiles(&root)?;
        Ok(Self { rx, root, profiles })
    }

    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.run())
    }

    pub async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                ProfileMsg::ListProfiles { reply } => {
                    let _ = reply.send(self.list_profiles());
                }
                ProfileMsg::GetProfile { name, reply } => {
                    let _ = reply.send(self.profile(&name).cloned());
                }
                ProfileMsg::GetModelConfig {
                    profile_name,
                    reply,
                } => {
                    let _ = reply.send(self.model_config(&profile_name));
                }
                ProfileMsg::GetPrompts {
                    profile_name,
                    reply,
                } => {
                    let _ = reply.send(
                        self.profile(&profile_name)
                            .map(|profile| profile.prompts.clone()),
                    );
                }
                ProfileMsg::GetTools {
                    profile_name,
                    reply,
                } => {
                    let _ = reply.send(
                        self.profile(&profile_name)
                            .map(|profile| profile.tools.clone()),
                    );
                }
            }
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
            .ok_or_else(|| SubsystemError::not_found("profile", name))
    }

    fn model_config(&self, profile_name: &str) -> SubsystemResult<FinalModelConfig> {
        let model = self.profile(profile_name)?.model.clone();
        let api_key = std::env::var(&model.api_key_env)
            .map_err(|_| SubsystemError::not_found("env", model.api_key_env.clone()))?;
        Ok(FinalModelConfig {
            provider: model.provider,
            model_name: model.model_name,
            api_key,
        })
    }
}

impl ProfileHandle {
    pub async fn list_profiles(&self) -> SubsystemResult<Vec<String>> {
        request(&self.tx, |reply| ProfileMsg::ListProfiles { reply }).await
    }

    pub async fn profile(&self, name: impl Into<String>) -> SubsystemResult<Profile> {
        request(&self.tx, |reply| ProfileMsg::GetProfile {
            name: name.into(),
            reply,
        })
        .await
    }

    pub async fn model_config(
        &self,
        profile_name: impl Into<String>,
    ) -> SubsystemResult<FinalModelConfig> {
        request(&self.tx, |reply| ProfileMsg::GetModelConfig {
            profile_name: profile_name.into(),
            reply,
        })
        .await
    }

    pub async fn prompts(
        &self,
        profile_name: impl Into<String>,
    ) -> SubsystemResult<PromptsConfigSection> {
        request(&self.tx, |reply| ProfileMsg::GetPrompts {
            profile_name: profile_name.into(),
            reply,
        })
        .await
    }

    pub async fn tools(
        &self,
        profile_name: impl Into<String>,
    ) -> SubsystemResult<ToolsConfigSection> {
        request(&self.tx, |reply| ProfileMsg::GetTools {
            profile_name: profile_name.into(),
            reply,
        })
        .await
    }
}

async fn request<T>(
    tx: &mpsc::Sender<ProfileMsg>,
    message: impl FnOnce(tokio::sync::oneshot::Sender<SubsystemResult<T>>) -> ProfileMsg,
) -> SubsystemResult<T> {
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    tx.send(message(reply_tx))
        .await
        .map_err(|_| SubsystemError::actor_dead(PROFILE_ACTOR))?;
    reply_rx
        .await
        .map_err(|_| SubsystemError::actor_dead(PROFILE_ACTOR))?
}

fn default_profiles_root() -> SubsystemResult<PathBuf> {
    Ok(dirs::home_dir()
        .ok_or_else(|| SubsystemError::internal("failed to determine home directory"))?
        .join(".prismagent")
        .join("profiles"))
}

fn load_profiles(root: &Path) -> SubsystemResult<HashMap<String, Profile>> {
    let mut profiles = HashMap::new();
    if root.exists() {
        for entry in std::fs::read_dir(root)? {
            let path = entry?.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                continue;
            }
            let profile = read_profile(&path)?;
            profiles.insert(profile.name.clone(), profile);
        }
    }
    if !profiles.contains_key(DEFAULT_PROFILE_NAME) {
        let profile: Profile = toml::from_str(DEFAULT_PROFILE).map_err(|error| {
            SubsystemError::invalid_input(format!("default profile is invalid: {error}"))
        })?;
        profiles.insert(profile.name.clone(), profile);
    }
    Ok(profiles)
}

fn read_profile(path: &Path) -> SubsystemResult<Profile> {
    let data = std::fs::read_to_string(path)
        .map_err(|error| SubsystemError::io(format!("{}: {error}", path.display())))?;
    let profile: Profile = toml::from_str(&data)
        .map_err(|error| SubsystemError::invalid_input(format!("{}: {error}", path.display())))?;
    let expected_name = path.file_stem().and_then(|stem| stem.to_str());
    if expected_name != Some(profile.name.as_str()) {
        return Err(SubsystemError::invalid_input(format!(
            "{}: profile name {} does not match file name {}",
            path.display(),
            profile.name,
            expected_name.unwrap_or("<invalid>")
        )));
    }
    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn loads_default_profile_when_directory_is_empty() {
        let root = std::env::temp_dir().join(format!("prismagent-profiles-{}", Uuid::now_v7()));
        let (tx, rx) = mpsc::channel::<ProfileMsg>(8);
        ProfileActor::from_root(rx, root).unwrap().spawn();
        let handle = ProfileHandle { tx };

        let names = handle.list_profiles().await.unwrap();
        assert_eq!(names, vec!["default".to_string()]);
        let profile = handle.profile("default").await.unwrap();
        assert_eq!(profile.name, "default");
        assert!(!profile.tools.yolo);
    }
}
