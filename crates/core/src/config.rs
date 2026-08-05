//! Plugin configuration, as written in a layout or passed to `zellij plugin -c`.

use std::collections::BTreeMap;

/// Process names that mark a pane as an agent when nothing is reporting on it.
pub const DEFAULT_AGENTS: [&str; 4] = ["claude", "codex", "aider", "gemini"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Lowercased names matched against pane titles for discovery.
    pub agents: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            agents: DEFAULT_AGENTS.iter().map(|name| name.to_string()).collect(),
        }
    }
}

impl Config {
    /// Reads the `agents` key, a comma-separated list. Anything unparseable
    /// falls back to the defaults: a typo in a layout should not leave the
    /// sidebar unable to recognise anything.
    pub fn from_map(configuration: &BTreeMap<String, String>) -> Self {
        let agents: Vec<String> = configuration
            .get("agents")
            .map(|raw| {
                raw.split(',')
                    .map(|name| name.trim().to_lowercase())
                    .filter(|name| !name.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        if agents.is_empty() {
            return Self::default();
        }
        Self { agents }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn defaults_cover_the_common_agents() {
        assert_eq!(Config::from_map(&map(&[])), Config::default());
        assert!(Config::default().agents.contains(&"claude".to_string()));
    }

    #[test]
    fn reads_and_normalises_a_list() {
        let config = Config::from_map(&map(&[("agents", "Claude, my-agent ,CODEX")]));
        assert_eq!(config.agents, vec!["claude", "my-agent", "codex"]);
    }

    #[test]
    fn an_empty_or_junk_list_falls_back_to_defaults() {
        assert_eq!(Config::from_map(&map(&[("agents", "")])), Config::default());
        assert_eq!(
            Config::from_map(&map(&[("agents", " , ")])),
            Config::default()
        );
    }
}
