//! Everything you could go to, and finding it by typing three letters.
//!
//! The sidebar is for watching; this is for moving. It lists every agent on the
//! machine, every session, and the ones that died and can come back — flattened
//! into one list, because when you want the api backend you do not want to first
//! decide whether it is a pane, a session or yesterday.

use crate::agent::{Agent, Status};

/// What picking an entry does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A pane, wherever it lives. Reaching one in another session costs a
    /// detach, which is what jumping means there.
    Pane { session: String, pane: u32 },
    /// A whole session. `dead` ones are Zellij's resurrectable sessions: the
    /// panes are gone but the layout comes back.
    Session { name: String, dead: bool },
    /// A session on another machine. Nothing to switch to — Zellij cannot show a
    /// pane it does not own — so going there means a pane here attached to it.
    Remote { host: String, session: String },
    /// A directory to start a row in. The one entry that is not somewhere to go
    /// but somewhere to *begin*: the row is opened there, template and all.
    Dir { path: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub glyph: char,
    /// What it is called.
    pub label: String,
    /// Where it is, shown after the label and dimmer.
    pub context: String,
    /// What typing is matched against: more than is displayed, so a project name
    /// finds an agent that does not carry it in its own name.
    pub search: String,
    pub target: Target,
}

/// A live session, and one that can be brought back.
const SESSION: char = '⊙';
const DEAD: char = '⊗';
/// A directory.
const DIR: char = '▸';

/// Where a new row could be started: the directories you actually go to, and
/// the ones already being worked in.
///
/// `frecent` is zoxide's list, best first, and it stays in that order — it has
/// already done the ranking that matters before anything is typed. Directories
/// an agent is in come after, since a place with a row in it is a place you can
/// reach through the sidebar anyway.
pub fn directories(frecent: &[String], agents: &[Agent], home: &str) -> Vec<Entry> {
    let mut paths: Vec<(String, &str)> = frecent
        .iter()
        .map(|path| (path.trim_end_matches('/').to_owned(), "recent"))
        .collect();

    for agent in agents.iter().filter(|agent| agent.host.is_empty()) {
        let root = match (agent.root.is_empty(), agent.cwd.is_empty()) {
            (false, _) => &agent.root,
            (true, false) => &agent.cwd,
            _ => continue,
        };
        paths.push((root.trim_end_matches('/').to_owned(), "in use"));
    }

    let mut seen: Vec<String> = Vec::new();
    paths
        .into_iter()
        .filter(|(path, _)| path.starts_with('/'))
        .filter(|(path, _)| {
            let fresh = !seen.contains(path);
            if fresh {
                seen.push(path.clone());
            }
            fresh
        })
        .map(|(path, why)| Entry {
            glyph: DIR,
            label: shorten(&path, home),
            context: why.to_owned(),
            // The whole path, so typing part of a parent finds it even when the
            // label has been shortened.
            search: path.clone(),
            target: Target::Dir { path },
        })
        .collect()
}

/// `~` for the home directory, because a list of absolute paths is a column of
/// the same eleven characters.
fn shorten(path: &str, home: &str) -> String {
    match path.strip_prefix(home).filter(|_| !home.is_empty()) {
        Some("") => "~".to_owned(),
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_owned(),
    }
}

/// The whole list, in the order it is worth seeing before anything is typed:
/// agents that want you, then the rest, then sessions, then the dead.
pub fn entries(
    agents: &[Agent],
    live_sessions: &[String],
    dead_sessions: &[String],
    current_session: &str,
) -> Vec<Entry> {
    let mut agents: Vec<&Agent> = agents.iter().collect();
    agents.sort_by_key(|agent| (agent.status, agent.session.clone(), agent.pane));

    let mut out: Vec<Entry> = agents
        .iter()
        .map(|agent| {
            let project = crate::project::display(if agent.root.is_empty() {
                &agent.cwd
            } else {
                &agent.root
            });
            let (elsewhere, where_) = if !agent.host.is_empty() {
                ("⇥", format!("{}:{}", agent.host, agent.session))
            } else if agent.session == current_session {
                ("", agent.session.clone())
            } else {
                ("⇢", agent.session.clone())
            };
            Entry {
                glyph: agent.status.glyph(),
                label: agent.label().to_owned(),
                context: format!("{elsewhere}{where_}"),
                search: format!("{} {project} {where_}", agent.label()),
                target: if agent.host.is_empty() {
                    Target::Pane {
                        session: agent.session.clone(),
                        pane: agent.pane,
                    }
                } else {
                    Target::Remote {
                        host: agent.host.clone(),
                        session: agent.session.clone(),
                    }
                },
            }
        })
        .collect();

    out.extend(live_sessions.iter().map(|name| {
        Entry {
            glyph: SESSION,
            label: name.clone(),
            context: if name == current_session {
                "here"
            } else {
                "session"
            }
            .to_owned(),
            search: format!("{name} session"),
            target: Target::Session {
                name: name.clone(),
                dead: false,
            },
        }
    }));
    out.extend(dead_sessions.iter().map(|name| Entry {
        glyph: DEAD,
        label: name.clone(),
        context: "resurrect".to_owned(),
        search: format!("{name} resurrect dead"),
        target: Target::Session {
            name: name.clone(),
            dead: true,
        },
    }));
    out
}

/// How well `needle` matches `haystack`, or `None` when it does not.
///
/// Subsequence matching, the thing everyone means by fuzzy: every letter has to
/// appear in order, and letters that arrive together or at the start of a word
/// count for more — so `api` prefers `api-server` to `capital`, and a shorter
/// name wins a tie because you were probably thinking of the shorter one.
pub fn score(haystack: &str, needle: &str) -> Option<i32> {
    let hay: Vec<char> = haystack.to_lowercase().chars().collect();
    let mut score = 0;
    let mut at = 0;
    let mut streak = 0;

    for want in needle.to_lowercase().chars().filter(|c| !c.is_whitespace()) {
        let found = at + hay[at..].iter().position(|have| *have == want)?;
        let boundary = found == 0 || matches!(hay[found - 1], ' ' | '-' | '_' | '/' | '.' | ':');
        streak = if found == at && at > 0 { streak + 1 } else { 0 };
        score += 1 + streak * 2 + i32::from(boundary) * 3;
        at = found + 1;
    }
    Some(score * 100 - hay.len() as i32)
}

/// The entries that match, best first. Ties keep the order they came in, so an
/// empty query leaves the list exactly as [`entries`] built it.
pub fn rank(entries: &[Entry], query: &str) -> Vec<usize> {
    let mut scored: Vec<(i32, usize)> = entries
        .iter()
        .enumerate()
        .filter_map(|(at, entry)| score(&entry.search, query).map(|score| (score, at)))
        .collect();

    scored.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
    scored.into_iter().map(|(_, at)| at).collect()
}

/// One line of the list: `<glyph> <label>   <context>`, padded to the width so
/// the contexts line up and the previous frame is covered.
pub fn line(entry: &Entry, width: usize) -> String {
    let context = crate::format::truncate(&entry.context, width.saturating_sub(4));
    let room = width.saturating_sub(3 + context.chars().count());
    let label = crate::format::truncate(&entry.label, room);
    let gap = " ".repeat(room.saturating_sub(label.chars().count()));

    crate::format::truncate(&format!("{} {label}{gap} {context}", entry.glyph), width)
}

/// Marks the status of an entry for colouring, so an agent that needs you looks
/// the same here as it does in the sidebar.
pub fn status(entry: &Entry) -> Status {
    match entry.glyph {
        glyph if glyph == Status::NeedsInput.glyph() => Status::NeedsInput,
        glyph if glyph == Status::Running.glyph() => Status::Running,
        glyph if glyph == Status::Done.glyph() => Status::Done,
        glyph if glyph == Status::Idle.glyph() => Status::Idle,
        SESSION => Status::Unknown,
        DIR => Status::Idle,
        _ => Status::Pane,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directories_come_from_zoxide_first_and_then_the_work() {
        let frecent = vec![
            "/home/pp/personal/agenttij".to_owned(),
            "/home/pp/work/api/".to_owned(),
            "not/absolute".to_owned(),
        ];
        let agents = vec![
            agent("main", 1, "/home/pp/personal/agenttij", Status::Running),
            agent("main", 2, "/home/pp/scratch", Status::Idle),
        ];

        let entries = directories(&frecent, &agents, "/home/pp");
        let labels: Vec<&str> = entries.iter().map(|entry| entry.label.as_str()).collect();
        assert_eq!(
            labels,
            vec!["~/personal/agenttij", "~/work/api", "~/scratch"],
            "zoxide's order kept, a trailing slash is the same place, \
             a directory already worked in comes after, and a relative line is not a path"
        );
        assert_eq!(entries[0].context, "recent");
        assert_eq!(entries[2].context, "in use");
        assert_eq!(
            entries[1].target,
            Target::Dir {
                path: "/home/pp/work/api".to_owned()
            }
        );
        // Typing a piece of the path still finds it, label or no label.
        assert!(score(&entries[0].search, "personal").is_some());
    }

    #[test]
    fn a_home_relative_label_is_only_for_home() {
        let frecent = vec!["/etc/nginx".to_owned(), "/home/pp".to_owned()];
        let entries = directories(&frecent, &[], "/home/pp");

        assert_eq!(entries[0].label, "/etc/nginx");
        assert_eq!(entries[1].label, "~");
        // No home to speak of leaves paths alone rather than eating the front.
        assert_eq!(directories(&frecent, &[], "")[0].label, "/etc/nginx");
    }

    fn agent(session: &str, pane: u32, cwd: &str, status: Status) -> Agent {
        Agent {
            session: session.into(),
            pane,
            status,
            cwd: cwd.into(),
            root: cwd.into(),
            ..Agent::default()
        }
    }

    #[test]
    fn a_letter_missing_is_no_match() {
        assert!(score("api-server", "apz").is_none());
        assert!(
            score("api-server", "").is_some(),
            "everything matches nothing"
        );
    }

    #[test]
    fn letters_together_and_at_a_word_start_are_worth_more() {
        let together = score("api-server", "api").expect("matches");
        let scattered = score("capital", "api").expect("matches");
        assert!(together > scattered, "{together} should beat {scattered}");
    }

    #[test]
    fn the_shorter_of_two_equal_matches_wins() {
        let short = score("api", "api").expect("matches");
        let long = score("api-gateway-service", "api").expect("matches");
        assert!(short > long);
    }

    #[test]
    fn ranking_keeps_the_given_order_when_nothing_is_typed() {
        let entries = entries(
            &[
                agent("main", 1, "/home/pp/web", Status::Idle),
                agent("main", 2, "/home/pp/api", Status::NeedsInput),
            ],
            &["main".to_owned()],
            &[],
            "main",
        );

        // Sorted by status first, so the one that needs you is already on top.
        assert_eq!(entries[0].label, "api");
        assert_eq!(rank(&entries, ""), vec![0, 1, 2]);
    }

    #[test]
    fn a_project_finds_an_agent_that_is_not_named_after_it() {
        let mut backend = agent("other", 3, "/home/pp/services/queue", Status::Running);
        backend.root = "acme".into();
        let entries = entries(&[backend], &[], &[], "main");

        let found = rank(&entries, "acme");
        assert_eq!(found, vec![0], "the project is searchable, not just shown");
        assert_eq!(entries[0].label, "queue");
        assert_eq!(entries[0].context, "⇢other", "and it says where it is");
    }

    #[test]
    fn an_agent_on_another_machine_is_reached_by_attaching() {
        let mut remote = agent("their-main", 4, "/srv/api", Status::Running);
        remote.host = "dev1".into();
        let entries = entries(&[remote], &[], &[], "mine");

        assert_eq!(
            entries[0].target,
            Target::Remote {
                host: "dev1".to_owned(),
                session: "their-main".to_owned()
            }
        );
        assert_eq!(entries[0].context, "⇥dev1:their-main");
        assert_eq!(rank(&entries, "dev1"), vec![0], "the machine is searchable");
    }

    #[test]
    fn sessions_come_after_agents_and_the_dead_come_last() {
        let entries = entries(
            &[agent("main", 1, "/home/pp/api", Status::Idle)],
            &["main".to_owned()],
            &["yesterday".to_owned()],
            "main",
        );

        assert_eq!(entries.len(), 3);
        assert_eq!(
            entries[2].target,
            Target::Session {
                name: "yesterday".to_owned(),
                dead: true
            }
        );
        assert_eq!(entries[1].context, "here", "the session you are in says so");
    }

    #[test]
    fn a_line_fits_the_width_it_is_given() {
        let entries = entries(
            &[agent("main", 1, "/home/pp/api", Status::Idle)],
            &[],
            &[],
            "x",
        );
        for width in [8, 20, 40] {
            assert!(line(&entries[0], width).chars().count() <= width);
        }
    }
}
