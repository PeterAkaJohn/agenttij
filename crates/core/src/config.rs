//! Plugin configuration, as written in a layout or passed to `zellij plugin -c`.

use std::collections::BTreeMap;

/// Process names that mark a pane as an agent when nothing is reporting on it.
pub const DEFAULT_AGENTS: [&str; 5] = ["claude", "codex", "opencode", "aider", "gemini"];

/// Which agents a sidebar lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    /// Every agent on the machine. Picking one in another session means
    /// detaching from this one.
    #[default]
    All,
    /// Only agents in this session, for a workspace layout where picking an
    /// agent should never move you out of the session you are sitting in.
    Session,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Lowercased names matched against pane titles for discovery.
    pub agents: Vec<String>,
    pub scope: Scope,
    /// Show only the selected agent's pane, parking the others out of sight
    /// instead of leaving them on screen.
    pub solo: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            agents: DEFAULT_AGENTS.iter().map(|name| name.to_string()).collect(),
            scope: Scope::default(),
            solo: false,
        }
    }
}

impl Config {
    /// Reads `agents` (a comma-separated list) and `scope`. Anything
    /// unparseable falls back to the default: a typo in a layout should not
    /// leave the sidebar unable to recognise anything.
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

        let scope = match configuration.get("scope").map(|raw| raw.trim()) {
            Some("session") => Scope::Session,
            _ => Scope::All,
        };

        let solo = configuration.get("solo").map(|raw| raw.trim()) == Some("true");

        let defaults = Self::default();
        Self {
            agents: if agents.is_empty() {
                defaults.agents
            } else {
                agents
            },
            scope,
            solo,
        }
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
    fn scope_defaults_to_every_session() {
        assert_eq!(Config::from_map(&map(&[])).scope, Scope::All);
        assert_eq!(
            Config::from_map(&map(&[("scope", "nonsense")])).scope,
            Scope::All
        );
    }

    #[test]
    fn scope_can_be_narrowed_to_this_session() {
        assert_eq!(
            Config::from_map(&map(&[("scope", "session")])).scope,
            Scope::Session
        );
        assert_eq!(
            Config::from_map(&map(&[("scope", " session ")])).scope,
            Scope::Session
        );
    }

    #[test]
    fn solo_is_off_unless_asked_for() {
        assert!(!Config::from_map(&map(&[])).solo);
        assert!(!Config::from_map(&map(&[("solo", "yes")])).solo);
        assert!(Config::from_map(&map(&[("solo", "true")])).solo);
        assert!(Config::from_map(&map(&[("solo", " true ")])).solo);
    }

    #[test]
    fn scope_and_agents_are_read_independently() {
        let config = Config::from_map(&map(&[("agents", "claude"), ("scope", "session")]));
        assert_eq!(config.agents, vec!["claude"]);
        assert_eq!(config.scope, Scope::Session);
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
