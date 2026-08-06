//! The unit the sidebar tracks: one coding agent, in one pane, in one session.

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
    /// Not an agent at all — a pane in the workspace, listed so it can be
    /// switched to. A shell you just opened lives here until it reports.
    Pane,
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
            Self::NeedsInput => '⚠',
            Self::Running => '◐',
            Self::Done => '✓',
            Self::Idle => '○',
            Self::Unknown => '?',
            Self::Pane => '·',
        }
    }

    /// Theme colour slot for this status. Zellij maps these onto the user's
    /// theme, so the sidebar stays legible in light and dark.
    pub fn color_slot(self) -> usize {
        match self {
            Self::NeedsInput => 3,
            Self::Running => 2,
            Self::Done => 1,
            Self::Idle | Self::Unknown | Self::Pane => 0,
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
    /// Short name from the pane's title, for entries that have no cwd to name
    /// them — a plain pane, or an agent found by process name.
    pub title: String,
}

impl Agent {
    /// Identity of the pane this agent runs in. Two reports with the same key
    /// are the same agent.
    pub fn key(&self) -> (&str, u32) {
        (self.session.as_str(), self.pane)
    }

    /// Short name for the list: the working directory's basename, then the
    /// pane's title, and the session name only as a last resort.
    pub fn label(&self) -> &str {
        self.cwd
            .rsplit('/')
            .find(|part| !part.is_empty())
            .or(Some(self.title.as_str()).filter(|title| !title.is_empty()))
            .unwrap_or(&self.session)
    }
}

/// Agents that have just become blocked on you.
///
/// Compares two snapshots rather than reacting to every scan, so a notification
/// fires once when an agent starts waiting — not once a second for as long as it
/// keeps waiting.
pub fn newly_blocked<'a>(previous: &[Agent], current: &'a [Agent]) -> Vec<&'a Agent> {
    current
        .iter()
        .filter(|agent| agent.status == Status::NeedsInput)
        .filter(|agent| {
            previous
                .iter()
                .find(|before| before.key() == agent.key())
                .is_none_or(|before| before.status != Status::NeedsInput)
        })
        .collect()
}

/// Orders the list: this session first, then by attention needed, then by pane.
///
/// The last key matters more than it looks. Rows must not move under the cursor
/// while you are using the sidebar, and two things would otherwise shuffle them:
/// Zellij hands us panes in a `HashMap`, whose iteration order is not stable
/// between updates, and ordering by recency would lift a row the moment its
/// agent reported — including the one you just pressed Enter on. Sorting by pane
/// id pins every row that is not actually changing status. Recency is still
/// visible, in the age column.
pub fn sort_for_display(agents: &mut [Agent], current_session: &str) {
    agents.sort_by(|left, right| {
        let key = |agent: &Agent| {
            (
                agent.session != current_session,
                agent.status,
                agent.session.clone(),
                agent.pane,
            )
        };
        key(left).cmp(&key(right))
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
            title: String::new(),
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
    fn label_falls_back_to_the_pane_title_then_the_session() {
        let mut pane = agent(Status::Pane, 0, "");
        pane.title = "nvim".into();
        assert_eq!(pane.label(), "nvim");

        assert_eq!(agent(Status::Unknown, 0, "").label(), "sess");
    }

    #[test]
    fn a_plain_pane_sorts_below_every_agent() {
        let mut plain = agent(Status::Pane, 900, "/shell");
        plain.title = "zsh".into();
        let mut agents = vec![plain, agent(Status::Idle, 1, "/idle")];

        sort_for_display(&mut agents, "sess");
        assert_eq!(agents[0].label(), "idle");
    }

    fn at_pane(status: Status, pane: u32, cwd: &str) -> Agent {
        Agent {
            pane,
            ..agent(status, 100, cwd)
        }
    }

    #[test]
    fn only_fresh_blocks_are_reported() {
        let waiting = at_pane(Status::NeedsInput, 1, "/a");
        let working = at_pane(Status::Running, 2, "/b");

        let before = vec![working.clone()];
        let after = vec![waiting.clone(), working.clone()];
        let still = vec![waiting.clone()];
        let none = vec![working.clone()];

        // First time it blocks: reported.
        let fresh = newly_blocked(&before, &after);
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].label(), "a");

        // Still blocked on the next scan: not reported again.
        assert_eq!(newly_blocked(&still, &still).len(), 0);

        // Nothing blocked at all.
        assert_eq!(newly_blocked(&none, &none).len(), 0);
    }

    /// An agent that answers and blocks again is worth a second mention.
    #[test]
    fn blocking_again_after_working_is_reported_again() {
        let before = vec![at_pane(Status::Running, 1, "/a")];
        let after = vec![at_pane(Status::NeedsInput, 1, "/a")];

        assert_eq!(newly_blocked(&before, &after).len(), 1);
    }

    #[test]
    fn attention_sorts_first() {
        let mut agents = vec![
            at_pane(Status::Running, 1, "/a"),
            at_pane(Status::NeedsInput, 2, "/b"),
            at_pane(Status::Done, 3, "/c"),
        ];
        sort_for_display(&mut agents, "sess");
        let labels: Vec<&str> = agents.iter().map(Agent::label).collect();
        assert_eq!(labels, vec!["b", "a", "c"]);
    }

    /// Rows must not move under the cursor just because Zellij handed us the
    /// panes in a different order, or because an agent reported.
    #[test]
    fn order_ignores_input_order_and_recency() {
        let rows = |mut agents: Vec<Agent>| {
            sort_for_display(&mut agents, "sess");
            agents.iter().map(|a| a.pane).collect::<Vec<_>>()
        };

        let a = at_pane(Status::Running, 1, "/a");
        let b = at_pane(Status::Running, 2, "/b");
        let c = Agent {
            reported_at: 9_999,
            ..at_pane(Status::Running, 3, "/c")
        };

        assert_eq!(rows(vec![a.clone(), b.clone(), c.clone()]), vec![1, 2, 3]);
        assert_eq!(rows(vec![c.clone(), a.clone(), b.clone()]), vec![1, 2, 3]);
        assert_eq!(rows(vec![b, c, a]), vec![1, 2, 3]);
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
