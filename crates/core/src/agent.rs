//! The unit the sidebar tracks: one coding agent, in one pane, in one session.

use std::cmp::Reverse;

/// What an agent is doing.
///
/// The variant order is the sidebar's sort order, deliberately: whatever wants
/// your attention belongs at the top of the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    /// Blocked on you — a permission prompt, or a question.
    NeedsInput,
    /// Working.
    Running,
    /// Finished its turn and is waiting for the next instruction.
    Done,
    /// Started, but has not reported anything since.
    Idle,
    /// Found by process name, with no hook reporting on it.
    Unknown,
}

impl Status {
    /// Parses the state word written by `hooks/agenttij-state.sh`.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "needs-input" => Some(Self::NeedsInput),
            "running" => Some(Self::Running),
            "done" => Some(Self::Done),
            "idle" => Some(Self::Idle),
            _ => None,
        }
    }

    pub fn glyph(self) -> char {
        match self {
            Self::NeedsInput => '●',
            Self::Running => '◐',
            Self::Done => '✓',
            Self::Idle => '○',
            Self::Unknown => '?',
        }
    }

    /// Theme colour slot for this status. Zellij maps these onto the user's
    /// theme, so the sidebar stays legible in light and dark.
    pub fn color_slot(self) -> usize {
        match self {
            Self::NeedsInput => 3,
            Self::Running => 2,
            Self::Done => 1,
            Self::Idle | Self::Unknown => 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agent {
    pub session: String,
    pub pane: u32,
    pub status: Status,
    /// Unix seconds of the last state report. `0` when discovered by process
    /// name, since we have no idea how long it has been there.
    pub reported_at: u64,
    /// Directory the agent is working in. Empty when discovered.
    pub cwd: String,
}

impl Agent {
    /// Identity of the pane this agent runs in. Two reports with the same key
    /// are the same agent.
    pub fn key(&self) -> (&str, u32) {
        (self.session.as_str(), self.pane)
    }

    /// Short name for the list: the working directory's basename, falling back
    /// to the session name when we have no cwd.
    pub fn label(&self) -> &str {
        self.cwd
            .rsplit('/')
            .find(|part| !part.is_empty())
            .unwrap_or(&self.session)
    }
}

/// Orders agents by attention needed, then most recently active first.
///
/// Agents in this session sort above agents elsewhere, so the rows you can
/// reach without detaching are the ones under the cursor first.
pub fn sort_for_display(agents: &mut [Agent], current_session: &str) {
    agents.sort_by_key(|agent| {
        (
            agent.session != current_session,
            agent.status,
            Reverse(agent.reported_at),
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(status: Status, reported_at: u64, cwd: &str) -> Agent {
        Agent {
            session: "sess".into(),
            pane: 1,
            status,
            reported_at,
            cwd: cwd.into(),
        }
    }

    #[test]
    fn label_is_the_cwd_basename() {
        assert_eq!(
            agent(Status::Idle, 0, "/home/pp/personal/agenttij").label(),
            "agenttij"
        );
        assert_eq!(agent(Status::Idle, 0, "/home/pp/x/").label(), "x");
    }

    #[test]
    fn label_falls_back_to_session_without_a_cwd() {
        assert_eq!(agent(Status::Unknown, 0, "").label(), "sess");
    }

    #[test]
    fn attention_sorts_above_recency() {
        let mut agents = vec![
            agent(Status::Running, 500, "/a"),
            agent(Status::NeedsInput, 1, "/b"),
            agent(Status::Running, 900, "/c"),
        ];
        sort_for_display(&mut agents, "sess");
        let labels: Vec<&str> = agents.iter().map(Agent::label).collect();
        assert_eq!(labels, vec!["b", "c", "a"]);
    }

    /// Reaching an agent in another session costs a detach, so those rows sort
    /// below everything local even when they are the ones shouting.
    #[test]
    fn this_session_sorts_above_other_sessions() {
        let mut elsewhere = agent(Status::NeedsInput, 900, "/remote");
        elsewhere.session = "other".into();
        let mut agents = vec![elsewhere, agent(Status::Idle, 1, "/local")];

        sort_for_display(&mut agents, "sess");
        let labels: Vec<&str> = agents.iter().map(Agent::label).collect();
        assert_eq!(labels, vec!["local", "remote"]);
    }
}
