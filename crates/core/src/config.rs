//! Plugin configuration, as written in a layout or passed to `zellij plugin -c`.

use crate::color::Colors;
use std::collections::BTreeMap;

/// What the sidebar calls itself in its pane frame. Without this the frame shows
/// the plugin's url, which is a filesystem path and tells you nothing.
pub const DEFAULT_TITLE: &str = "agents";

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
    /// Pane frame title.
    pub title: String,
    /// Status glyph colours.
    pub colors: Colors,
    /// The `colors` string as written, to pass on to a peek instance.
    pub colors_raw: String,
    /// Command run when an agent becomes blocked on you, as words to exec. The
    /// agent's name is appended. Empty means no notification.
    pub notify: Vec<String>,
    /// Set on a peek instance: the pane it mirrors, as `<session>:<pane>`. A
    /// sidebar with this set *is* the peek — it renders that pane and closes on
    /// any key, which a command pane cannot do because it cannot read one.
    pub peek: Option<(String, u32)>,
    /// Set on a help instance: this sidebar is the keybind list, and closes on
    /// any key.
    pub help: bool,
    /// Name each pane in a row `<row> <n>/<total>`, so a pane says where it sits
    /// without the sidebar being on screen. On unless turned off.
    pub position: bool,
    /// Show only the selected agent's pane, parking the others out of sight
    /// instead of leaving them on screen.
    pub solo: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            agents: DEFAULT_AGENTS.iter().map(|name| name.to_string()).collect(),
            scope: Scope::default(),
            title: DEFAULT_TITLE.to_string(),
            colors: Colors::default(),
            colors_raw: String::new(),
            notify: Vec::new(),
            peek: None,
            help: false,
            position: true,
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
        let help = configuration.get("help").map(|raw| raw.trim()) == Some("true");
        let position = configuration.get("position").map(|raw| raw.trim()) != Some("false");

        // Not `title`: Zellij keeps that key for itself and it never reaches the
        // plugin — measured by dumping a launched plugin's configuration, which
        // showed every other key and not this one.
        let title = configuration
            .get("pane_title")
            .map(|raw| raw.trim())
            .filter(|raw| !raw.is_empty())
            .unwrap_or(DEFAULT_TITLE)
            .to_string();

        let notify: Vec<String> = configuration
            .get("notify")
            .map(|raw| raw.split_whitespace().map(str::to_owned).collect())
            .unwrap_or_default();

        let colors_raw = configuration.get("colors").cloned().unwrap_or_default();
        let colors = if colors_raw.is_empty() {
            Colors::default()
        } else {
            Colors::from_pairs(&colors_raw)
        };

        let peek = configuration.get("peek").and_then(|raw| {
            let (session, pane) = raw.trim().rsplit_once(':')?;
            Some((session.to_owned(), pane.trim().parse().ok()?))
        });

        let defaults = Self::default();
        Self {
            agents: if agents.is_empty() {
                defaults.agents
            } else {
                agents
            },
            scope,
            title,
            colors,
            colors_raw,
            notify,
            peek,
            help,
            position,
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
    fn the_pane_title_has_a_default_and_can_be_set() {
        assert_eq!(Config::from_map(&map(&[])).title, "agents");
        assert_eq!(
            Config::from_map(&map(&[("pane_title", "")])).title,
            "agents"
        );
        assert_eq!(
            Config::from_map(&map(&[("pane_title", " sessions ")])).title,
            "sessions"
        );
        // `title` is Zellij's own key and never arrives, so it must not be ours.
        assert_eq!(
            Config::from_map(&map(&[("title", "ignored")])).title,
            "agents"
        );
    }

    #[test]
    fn colours_default_and_can_be_overridden_per_status() {
        use crate::agent::Status;

        assert_eq!(Config::from_map(&map(&[])).colors, Colors::default());

        let config = Config::from_map(&map(&[("colors", "running=#ff8800")]));
        assert_eq!(config.colors.of(Status::Running), "38;2;255;136;0");
        assert_eq!(config.colors.of(Status::Done), "32", "default kept");
    }

    #[test]
    fn notifications_are_off_until_a_command_is_given() {
        assert!(Config::from_map(&map(&[])).notify.is_empty());
        assert!(Config::from_map(&map(&[("notify", "   ")]))
            .notify
            .is_empty());
        assert_eq!(
            Config::from_map(&map(&[("notify", "notify-send -u critical")])).notify,
            vec!["notify-send", "-u", "critical"]
        );
    }

    #[test]
    fn a_peek_target_is_a_session_and_a_pane() {
        assert_eq!(Config::from_map(&map(&[])).peek, None);
        assert_eq!(
            Config::from_map(&map(&[("peek", "main:7")])).peek,
            Some(("main".to_string(), 7))
        );
        // Session names may contain colons; the pane is the last field.
        assert_eq!(
            Config::from_map(&map(&[("peek", "od:d:12")])).peek,
            Some(("od:d".to_string(), 12))
        );
        assert_eq!(Config::from_map(&map(&[("peek", "main:x")])).peek, None);
        assert_eq!(Config::from_map(&map(&[("peek", "main")])).peek, None);
    }

    #[test]
    fn pane_positions_are_on_unless_turned_off() {
        assert!(Config::from_map(&map(&[])).position);
        assert!(Config::from_map(&map(&[("position", "true")])).position);
        assert!(!Config::from_map(&map(&[("position", "false")])).position);
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
