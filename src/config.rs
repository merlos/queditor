use anyhow::{Context, Result, bail};
use ratatui::style::Color;
use serde::Deserialize;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub theme: String,
    pub keybindings: String,
    /// `mock` or a genai model identifier (prefer explicit provider namespaces).
    pub model: String,
    pub models: Vec<String>,
    pub persist_sessions: bool,
    pub recovery: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: 1,
            theme: "mocha".into(),
            keybindings: "vscode".into(),
            model: "mock".into(),
            models: vec!["mock".into()],
            persist_sessions: false,
            recovery: true,
        }
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
        validate_model(&c.model)?;
        for model in &c.models {
            validate_model(model)?;
        }
        Ok(c)
    }
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
                ("ctrl+a", "select_all"),
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
    fn partial_config_uses_defaults() {
        let c: Config = serde_yaml_ng::from_str("theme: daylight").unwrap();
        assert_eq!(c.keybindings, "vscode");
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
