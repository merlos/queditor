use anyhow::{Context, Result, bail};
use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub theme: String,
    pub keybindings: String,
    /// Stable display name of the last selected built-in or custom agent.
    pub agent: String,
    /// Last operating mode selected for each agent name.
    pub agent_modes: BTreeMap<String, AgentMode>,
    /// `mock` or a genai model identifier (prefer explicit provider namespaces).
    pub model: String,
    pub models: Vec<String>,
    pub providers: BTreeMap<String, Provider>,
    pub persist_sessions: bool,
    pub recovery: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: 1,
            theme: "mocha".into(),
            keybindings: "vscode".into(),
            agent: "Planner".into(),
            agent_modes: BTreeMap::new(),
            model: "mock".into(),
            models: vec!["mock".into()],
            providers: BTreeMap::new(),
            persist_sessions: false,
            recovery: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AgentMode {
    #[default]
    Chat,
    Agent,
}
impl AgentMode {
    /// Shared mode order for agent/mode rotation and the current-agent shortcut.
    /// Register new modes here so navigation does not need mode-specific branches.
    pub const ALL: &'static [Self] = &[Self::Chat, Self::Agent];

    pub fn next(self) -> Self {
        let index = Self::ALL
            .iter()
            .position(|mode| *mode == self)
            .expect("Registered agent mode");
        Self::ALL[(index + 1) % Self::ALL.len()]
    }
}
impl Config {
    pub fn load() -> Result<Self> {
        let path = config_dir().join("config.yaml");
        let c: Self = if path.exists() {
            serde_yaml_ng::from_str(&std::fs::read_to_string(&path)?)
                .context("Invalid config.yaml")?
        } else {
            Self::default()
        };
        if c.schema_version != 1 {
            bail!("Unsupported config schema version");
        }
        if !matches!(c.theme.as_str(), "mocha" | "daylight") {
            bail!("Unknown theme; use mocha or daylight");
        }
        if c.agent.trim().is_empty() || c.agent.len() > 200 || c.agent.chars().any(char::is_control)
        {
            bail!("Invalid agent name");
        }
        c.validate_model_ref(&c.model)?;
        for model in &c.models {
            c.validate_model_ref(model)?;
        }
        for (id, provider) in &c.providers {
            validate_provider(id, provider)?;
        }
        Ok(c)
    }

    pub fn validate_model_ref(&self, model: &str) -> Result<()> {
        if let Some((provider, name)) = model.split_once("::")
            && self.providers.contains_key(provider)
        {
            return validate_provider_model(name);
        }
        validate_model(model)
    }

    pub fn save(&self) -> Result<()> {
        let directory = config_dir();
        std::fs::create_dir_all(&directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
        }
        let path = directory.join("config.yaml");
        if path.exists() && std::fs::symlink_metadata(&path)?.file_type().is_symlink() {
            bail!("Config symlinks are not supported");
        }
        let bytes = serde_yaml_ng::to_string(self)?;
        let mut temporary = tempfile::NamedTempFile::new_in(&directory)?;
        use std::io::Write;
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;
        #[cfg(unix)]
        temporary
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
        temporary.write_all(bytes.as_bytes())?;
        temporary.as_file().sync_all()?;
        temporary.persist(path).map_err(|e| e.error)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    #[serde(default)]
    pub name: String,
    #[serde(default = "openai_compatible")]
    pub protocol: String,
    pub base_url: String,
    #[serde(default)]
    pub credential: Credential,
    #[serde(default)]
    pub models: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Credential {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keyring: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

fn openai_compatible() -> String {
    "openai_compatible".into()
}

fn validate_provider(id: &str, provider: &Provider) -> Result<()> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        bail!("Invalid provider ID: {id}");
    }
    if !matches!(
        provider.protocol.as_str(),
        "openai" | "openai_compatible" | "anthropic" | "gemini" | "openrouter"
    ) {
        bail!("Provider {id} uses an unsupported protocol");
    }
    crate::provider::normalize_endpoint(&provider.base_url)?;
    for model in &provider.models {
        validate_provider_model(model)?;
    }
    let credential_sources = [
        provider.credential.env.is_some(),
        provider.credential.keyring.is_some(),
        provider.credential.file.is_some(),
    ]
    .into_iter()
    .filter(|set| *set)
    .count();
    if credential_sources != 1 {
        bail!("Provider {id} must configure exactly one credential source");
    }
    if let Some(env) = &provider.credential.env
        && (env.is_empty()
            || !env
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
    {
        bail!("Provider {id} has an invalid environment variable name");
    }
    for name in [
        provider.credential.keyring.as_deref(),
        provider.credential.file.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if name.is_empty()
            || name.len() > 64
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        {
            bail!("Provider {id} has an invalid credential reference");
        }
    }
    Ok(())
}

pub fn validate_provider_model(model: &str) -> Result<()> {
    if model.trim().is_empty()
        || model.len() > 200
        || model.chars().any(char::is_whitespace)
        || model.contains("::")
    {
        bail!("Invalid provider model identifier");
    }
    Ok(())
}
pub fn validate_model(model: &str) -> Result<()> {
    if model.trim().is_empty() || model.len() > 200 || model.chars().any(char::is_whitespace) {
        bail!("Invalid model identifier");
    }
    if let Some((namespace, name)) = model.split_once("::")
        && (name.is_empty()
            || (genai::adapter::AdapterKind::from_lower_str(namespace).is_none()
                && !matches!(
                    namespace,
                    "zai_coding" | "baidu_coding_openai" | "baidu_coding_anthropic"
                )))
    {
        bail!("Unknown provider namespace or empty model name");
    }
    if model != "mock" {
        genai::adapter::AdapterKind::from_model(model)
            .context("Unknown provider/model namespace")?;
    }
    Ok(())
}
pub fn config_dir() -> PathBuf {
    #[cfg(test)]
    return test_config_dir();
    #[cfg(all(windows, not(test)))]
    {
        return std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("USERPROFILE")
                    .map(PathBuf::from)
                    .map(|home| home.join("AppData/Roaming"))
            })
            .unwrap_or_else(|| PathBuf::from("."))
            .join("queditor");
    }
    #[cfg(all(not(windows), not(test)))]
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(".config")
        })
        .join("queditor")
}

#[cfg(test)]
pub(crate) fn test_config_dir() -> PathBuf {
    // Each test thread owns its files: parallel saves must not collide or touch
    // the user's configuration, bindings, agents, or credentials.
    thread_local! {
        static DIRECTORY: tempfile::TempDir = tempfile::tempdir().expect("Test config directory");
    }
    DIRECTORY.with(|directory| directory.path().join("config"))
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Bindings {
    pub editor: BTreeMap<String, String>,
}
impl Bindings {
    pub fn load(profile: &str) -> Result<Self> {
        if profile.contains('/') || profile.contains('\\') || profile.contains("..") {
            bail!("Invalid binding profile name");
        }
        let mut b = Self::default();
        let defaults = if profile == "emacs" {
            vec![
                ("ctrl+b", "left"),
                ("ctrl+f", "right"),
                ("ctrl+p", "up"),
                ("ctrl+n", "down"),
                ("ctrl+a", "home"),
                ("ctrl+e", "end"),
                ("ctrl+k", "kill_line"),
                ("ctrl+d", "delete"),
                ("ctrl+h", "backspace"),
                ("ctrl+space", "mark"),
                ("ctrl+w", "cut"),
                ("alt+w", "copy"),
                ("ctrl+y", "paste"),
                ("ctrl+z", "undo"),
            ]
        } else {
            vec![
                ("ctrl+a", "home"),
                ("ctrl+e", "end"),
                ("ctrl+k", "kill_line"),
                ("ctrl+c", "copy"),
                ("ctrl+x", "cut"),
                ("ctrl+v", "paste"),
                ("ctrl+z", "undo"),
                ("ctrl+y", "redo"),
                ("ctrl+shift+z", "redo"),
            ]
        };
        if profile != "emacs"
            && profile != "vscode"
            && !config_dir()
                .join("keybindings")
                .join(format!("{profile}.yaml"))
                .exists()
        {
            bail!("Unknown binding profile: {profile}");
        }
        b.editor
            .extend(defaults.into_iter().map(|(k, v)| (k.into(), v.into())));
        let path = config_dir()
            .join("keybindings")
            .join(format!("{profile}.yaml"));
        if path.exists() {
            let custom: Self = serde_yaml_ng::from_str(&std::fs::read_to_string(path)?)?;
            for action in custom.editor.values() {
                if !matches!(
                    action.as_str(),
                    "left"
                        | "right"
                        | "up"
                        | "down"
                        | "home"
                        | "end"
                        | "delete"
                        | "backspace"
                        | "kill_line"
                        | "mark"
                        | "select_all"
                        | "cut"
                        | "copy"
                        | "paste"
                        | "undo"
                        | "redo"
                        | "none"
                ) {
                    bail!("Unknown editor action: {action}");
                }
            }
            b.editor.extend(custom.editor);
        }
        Ok(b)
    }
}

#[derive(Clone, Copy)]
pub struct Theme {
    pub bg: Color,
    /// Raised, borderless editor/input/user-message surface.
    pub surface: Color,
    pub fg: Color,
    pub muted: Color,
    pub accent: Color,
    pub selection: Color,
    pub green: Color,
    pub red: Color,
    pub yellow: Color,
}
impl Theme {
    pub fn named(name: &str) -> Self {
        if name == "daylight" {
            Self {
                bg: Color::Rgb(250, 248, 240),
                surface: Color::Rgb(255, 254, 250),
                fg: Color::Rgb(40, 44, 52),
                muted: Color::Rgb(100, 104, 115),
                accent: Color::Rgb(75, 60, 160),
                selection: Color::Rgb(212, 221, 245),
                green: Color::Rgb(35, 110, 60),
                red: Color::Rgb(175, 40, 45),
                yellow: Color::Rgb(140, 90, 10),
            }
        } else {
            Self {
                bg: Color::Rgb(30, 30, 46),
                surface: Color::Rgb(49, 50, 68),
                fg: Color::Rgb(205, 214, 244),
                muted: Color::Rgb(127, 132, 156),
                accent: Color::Rgb(203, 166, 247),
                selection: Color::Rgb(69, 71, 90),
                green: Color::Rgb(166, 227, 161),
                red: Color::Rgb(243, 139, 168),
                yellow: Color::Rgb(249, 226, 175),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_config_is_isolated_and_preferences_round_trip() {
        let original = Config::load().unwrap();
        let threads = ["Planner", "Blogger"].map(|agent| {
            std::thread::spawn(move || {
                let mut config = Config::load().unwrap();
                assert_eq!(config.agent, Config::default().agent);
                config.agent = agent.into();
                config.agent_modes.insert(agent.into(), AgentMode::Agent);
                config.save().unwrap();
                let restored = Config::load().unwrap();
                assert_eq!(restored.agent, agent);
                assert_eq!(restored.agent_modes[agent], AgentMode::Agent);
                config_dir()
            })
        });
        let [first, second] = threads.map(|thread| thread.join().unwrap());
        assert_ne!(first, second);
        assert_ne!(first, config_dir());
        let restored = Config::load().unwrap();
        assert_eq!(restored.agent, original.agent);
        assert_eq!(restored.agent_modes, original.agent_modes);
    }

    #[test]
    fn partial_config_uses_defaults() {
        let c: Config = serde_yaml_ng::from_str("theme: daylight").unwrap();
        assert_eq!(c.keybindings, "vscode");
        assert_eq!(c.agent, "Planner");
        assert_eq!(c.schema_version, 1);
    }
    #[test]
    fn rejects_unknown_fields() {
        assert!(serde_yaml_ng::from_str::<Config>("typo: true").is_err());
    }
    #[test]
    fn validates_provider_namespaces_and_keeps_mock_default() {
        assert_eq!(Config::default().model, "mock");
        for model in [
            "openai::gpt-4o-mini",
            "anthropic::claude-sonnet-4-5",
            "gemini::gemini-2.5-flash",
            "ollama::llama3.2",
            "genai_1::test",
        ] {
            assert!(validate_model(model).is_ok(), "{model}");
        }
        assert!(validate_model("typo::model").is_err());
        assert!(validate_model("openai::").is_err());
    }
}
