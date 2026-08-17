//! Rows that belong to the same codebase, on one line.
//!
//! A row is a group of panes; a project is a group of rows. Someone with four
//! repositories open has four things to think about, not fifteen — so the
//! sidebar folds each project down to a line that says what is happening inside
//! it, and opens the one you are working in.

use crate::agent::{Agent, Kind, Status};
use std::collections::{BTreeMap, BTreeSet};

/// Where a row's project came from before anyone named it: the git root the hook
/// resolved, falling back to the working directory when nothing did, and to
/// nothing at all for a pane we know neither for.
///
/// Panes with no directory of their own therefore share one nameless project,
/// which is right: they are the ones we cannot place.
pub fn root(agent: &Agent) -> &str {
    if agent.root.is_empty() {
        &agent.cwd
    } else {
        &agent.root
    }
}

/// The project a row belongs to: what you called it, or where it lives if you
/// never did.
///
/// This is what makes two repositories one project. A front end and a back end
/// have different git roots and no shared parent worth grouping by, so the only
/// thing that can join them is you saying they are the same thing — and once
/// both roots carry that name, every agent started in either lands in it without
/// being told again.
pub fn key<'a>(agent: &'a Agent, names: &'a BTreeMap<String, String>) -> &'a str {
    let root = root(agent);
    names.get(root).map(String::as_str).unwrap_or(root)
}

/// What a project is called on screen: the last part of its path, or the name
/// itself once it has one.
///
/// The same answer `Agent::label` gives a header, and the one that matters when
/// deciding whether two projects are "the same name" — because the name someone
/// compares is the one they can see, not the key underneath it.
pub fn display(key: &str) -> &str {
    key.rsplit('/').find(|part| !part.is_empty()).unwrap_or(key)
}

/// Splices a header above each set of rows sharing a project, and drops the rows
/// of folded ones.
///
/// Headers appear only when there is more than one project to tell apart. With
/// one project every header is a line the list did not have to spare, and the
/// sidebar is usually 20 columns of a screen someone is working in.
///
/// Order follows the rows, which arrive sorted attention-first, so the project
/// holding whatever needs you is the project at the top. A pane belongs to the
/// project of the row above it — the list is already in that shape, and reading
/// it that way means a pane never has to carry a project of its own.
pub fn group(
    rows: Vec<Agent>,
    folded: &BTreeSet<String>,
    names: &BTreeMap<String, String>,
) -> Vec<Agent> {
    let mut order: Vec<String> = Vec::new();
    for row in rows.iter().filter(|row| row.kind == Kind::Row) {
        let key = key(row, names).to_owned();
        if !order.contains(&key) {
            order.push(key);
        }
    }
    if order.len() < 2 {
        return rows;
    }

    let mut out = Vec::with_capacity(rows.len() + order.len());
    for project in &order {
        let folded = folded.contains(project);
        out.push(header(project, &rows, folded, names));
        if folded {
            continue;
        }

        let mut inside = false;
        for row in &rows {
            match row.kind {
                Kind::Row => inside = key(row, names) == project,
                // Belongs to whatever row it was listed under.
                Kind::Pane { .. } => {}
                Kind::Project { .. } => continue,
            }
            if inside {
                let mut member = row.clone();
                member.depth += 1;
                out.push(member);
            }
        }
    }
    out
}

/// One line standing for a project: its name, how many rows it holds, and the
/// worst thing happening in it.
///
/// The status is the minimum because [`Status`] is ordered by how much it wants
/// you — a folded project with a blocked agent inside has to say so, or folding
/// it would mean not being told.
fn header(project: &str, rows: &[Agent], folded: bool, names: &BTreeMap<String, String>) -> Agent {
    let members = || {
        rows.iter()
            .filter(|row| row.kind == Kind::Row && key(row, names) == project)
    };
    Agent {
        // No session and no pane: a header is not somewhere you can go, and the
        // cursor addresses it by project rather than by pane.
        status: members()
            .map(|row| row.status)
            .min()
            .unwrap_or(Status::Pane),
        panes: members().count(),
        // `label` names a row after its directory's last part, which is exactly
        // what a project should be called. Panes we know no directory for share
        // one project that has no name, so it is given one.
        cwd: project.to_owned(),
        title: if project.is_empty() {
            "other".into()
        } else {
            String::new()
        },
        root: project.to_owned(),
        kind: Kind::Project { folded },
        ..Agent::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(root: &str, pane: u32, status: Status) -> Agent {
        Agent {
            session: "here".into(),
            pane,
            status,
            cwd: root.into(),
            root: root.into(),
            ..Agent::default()
        }
    }

    fn names(rows: &[Agent]) -> Vec<String> {
        rows.iter()
            .map(|row| match row.kind {
                Kind::Project { folded } => {
                    format!(
                        "{} {}{}",
                        if folded { "▸" } else { "▾" },
                        row.label(),
                        row.panes
                    )
                }
                _ => format!("  {}:{}", row.label(), row.pane),
            })
            .collect()
    }

    #[test]
    fn one_project_gets_no_header() {
        let rows = vec![
            row("/home/pp/api", 1, Status::Idle),
            row("/home/pp/api", 2, Status::Done),
        ];
        let out = group(rows.clone(), &BTreeSet::new(), &BTreeMap::new());

        assert_eq!(out, rows, "a header would cost a line and tell you nothing");
    }

    #[test]
    fn rows_gather_under_their_project_in_the_order_they_arrived() {
        let rows = vec![
            row("/home/pp/api", 1, Status::NeedsInput),
            row("/home/pp/dotfiles", 2, Status::Idle),
            row("/home/pp/api", 3, Status::Running),
        ];
        let out = group(rows, &BTreeSet::new(), &BTreeMap::new());

        assert_eq!(
            names(&out),
            vec![
                "▾ api2",
                "  api:1",
                "  api:3",
                "▾ dotfiles1",
                "  dotfiles:2",
            ]
        );
        assert_eq!(out[1].depth, 1, "rows are indented under their project");
    }

    #[test]
    fn a_header_carries_the_worst_status_inside_it() {
        let rows = vec![
            row("/home/pp/api", 1, Status::Done),
            row("/home/pp/api", 2, Status::NeedsInput),
            row("/home/pp/dotfiles", 3, Status::Idle),
        ];
        let out = group(rows, &BTreeSet::new(), &BTreeMap::new());

        assert_eq!(
            out[0].status,
            Status::NeedsInput,
            "folding must not hide it"
        );
        assert_eq!(out[3].status, Status::Idle);
    }

    #[test]
    fn folding_a_project_hides_its_rows_and_their_panes() {
        let mut pane = row("/home/pp/api", 9, Status::Pane);
        pane.kind = Kind::Pane { last: true };
        let rows = vec![
            row("/home/pp/api", 1, Status::Running),
            pane,
            row("/home/pp/dotfiles", 2, Status::Idle),
        ];
        let folded = BTreeSet::from(["/home/pp/api".to_owned()]);

        assert_eq!(
            names(&group(rows, &folded, &BTreeMap::new())),
            vec!["▸ api1", "▾ dotfiles1", "  dotfiles:2"]
        );
    }

    #[test]
    fn two_repositories_under_one_name_are_one_project() {
        let rows = vec![
            row("/home/pp/acme-frontend", 1, Status::NeedsInput),
            row("/home/pp/acme-backend", 2, Status::Running),
            row("/home/pp/dotfiles", 3, Status::Idle),
        ];
        let called_acme = BTreeMap::from([
            ("/home/pp/acme-frontend".to_owned(), "acme".to_owned()),
            ("/home/pp/acme-backend".to_owned(), "acme".to_owned()),
        ]);

        assert_eq!(
            names(&group(rows, &BTreeSet::new(), &called_acme)),
            vec![
                "▾ acme2",
                "  acme-frontend:1",
                "  acme-backend:2",
                "▾ dotfiles1",
                "  dotfiles:3",
            ],
            "the rows keep their own names; the project takes the one you gave it"
        );
    }

    #[test]
    fn a_project_is_called_what_a_header_calls_it() {
        assert_eq!(display("/home/pp/personal/agenttij"), "agenttij");
        assert_eq!(display("/home/pp/dotfiles/"), "dotfiles");
        assert_eq!(
            display("acme"),
            "acme",
            "a name is already what it is called"
        );
        assert_eq!(display(""), "");
    }

    #[test]
    fn a_row_with_no_directory_still_lands_somewhere() {
        let rows = vec![
            Agent {
                session: "here".into(),
                pane: 1,
                ..Agent::default()
            },
            row("/home/pp/api", 2, Status::Idle),
        ];
        let out = group(rows, &BTreeSet::new(), &BTreeMap::new());

        assert_eq!(out.len(), 4, "two headers, two rows");
        assert_eq!(
            out[0].label(),
            "other",
            "a project with no directory still needs a name"
        );
    }
}
