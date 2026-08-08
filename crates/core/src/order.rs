//! Keeping the list in the order you put it in.
//!
//! The sidebar sorts by attention, which is right until you have an opinion.
//! Once you move something, that opinion wins for everything you have moved and
//! the sort keeps arranging the rest — so a project you dragged to the top stays
//! at the top, and an agent that appears later still lands somewhere sensible
//! instead of at a position nobody chose.

use std::collections::{BTreeMap, BTreeSet};

/// One remembered project, one remembered row inside one, and one project you
/// left folded.
const PROJECT: &str = "p";
const ROW: &str = "r";
const FOLDED: &str = "f";
const NAMED: &str = "n";
const HOST: &str = "h";

/// How you left the sidebar: what order things were in, and what was folded away.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Arrangement {
    /// Projects in the order you put them in.
    pub projects: Vec<String>,
    /// Each project's rows in the order you put them in, by project root.
    pub rows: BTreeMap<String, Vec<(String, u32)>>,
    /// Projects folded down to their header line, by project root.
    pub folded: BTreeSet<String>,
    /// What you called a project, by the git root it was called that from. Two
    /// roots under one name are one project — which is how a front end and a
    /// back end in separate repositories become the thing you actually work on.
    pub names: BTreeMap<String, String>,
    /// Machines to watch besides this one. Kept here rather than only in a
    /// layout because which boxes you care about changes during a day, and
    /// editing a layout file to say so is not something anyone does twice.
    pub hosts: Vec<String>,
}

/// The order, as a file.
///
/// Line based and tab separated like the state files, for the same reason: it is
/// read by `cat` and written by `printf`, and anything that cannot be read is
/// skipped rather than throwing the rest away.
pub fn encode(arrangement: &Arrangement) -> String {
    let mut out = String::new();
    for project in &arrangement.projects {
        out.push_str(&format!("{PROJECT}\t{project}\n"));
    }
    for (project, rows) in &arrangement.rows {
        for (session, pane) in rows {
            out.push_str(&format!("{ROW}\t{project}\t{session}\t{pane}\n"));
        }
    }
    for project in &arrangement.folded {
        out.push_str(&format!("{FOLDED}\t{project}\n"));
    }
    for (root, name) in &arrangement.names {
        out.push_str(&format!("{NAMED}\t{root}\t{name}\n"));
    }
    for host in &arrangement.hosts {
        out.push_str(&format!("{HOST}\t{host}\n"));
    }
    out
}

/// Reads back what [`encode`] wrote. A line that makes no sense is dropped: a
/// half-written file should cost you an arrangement, not a working sidebar.
pub fn decode(text: &str) -> Arrangement {
    let mut out = Arrangement::default();

    for line in text.lines() {
        let mut fields = line.split('\t');
        let kind = fields.next();
        match kind {
            Some(PROJECT) | Some(FOLDED) => {
                let Some(project) = fields.next().filter(|project| !project.is_empty()) else {
                    continue;
                };
                if kind == Some(FOLDED) {
                    out.folded.insert(project.to_owned());
                } else {
                    out.projects.push(project.to_owned());
                }
            }
            Some(HOST) => {
                if let Some(host) = fields.next().filter(|host| !host.is_empty()) {
                    out.hosts.push(host.to_owned());
                }
            }
            Some(NAMED) => {
                let (Some(root), Some(name)) = (fields.next(), fields.next()) else {
                    continue;
                };
                if !root.is_empty() && !name.is_empty() {
                    out.names.insert(root.to_owned(), name.to_owned());
                }
            }
            Some(ROW) => {
                let (Some(project), Some(session), Some(pane)) =
                    (fields.next(), fields.next(), fields.next())
                else {
                    continue;
                };
                if let Ok(pane) = pane.trim().parse() {
                    out.rows
                        .entry(project.to_owned())
                        .or_default()
                        .push((session.to_owned(), pane));
                }
            }
            _ => {}
        }
    }
    out
}

/// Puts `items` in the remembered order, with anything unremembered kept in the
/// order it arrived and placed after.
///
/// Unknown keys in `order` are skipped rather than treated as an error: a row
/// you moved and then closed should not leave a hole, or bring the list back in
/// a different shape when a pane happens to reuse its id.
pub fn arrange<T, K: PartialEq>(items: Vec<T>, order: &[K], key: impl Fn(&T) -> K) -> Vec<T> {
    if order.is_empty() {
        return items;
    }

    let mut left: Vec<Option<T>> = items.into_iter().map(Some).collect();
    let mut out = Vec::with_capacity(left.len());
    for wanted in order {
        let found = left
            .iter_mut()
            .find(|slot| slot.as_ref().is_some_and(|item| key(item) == *wanted));
        if let Some(slot) = found {
            out.extend(slot.take());
        }
    }
    out.extend(left.into_iter().flatten());
    out
}

/// Moves one entry one place through the remembered order.
///
/// `natural` is what is on screen right now, which is what the person moving
/// something is looking at — so the first move starts from that rather than from
/// an empty memory, and everything keeps its place except the one thing that
/// moved.
pub fn shift<K: Clone + PartialEq>(order: &mut Vec<K>, natural: &[K], key: &K, down: bool) {
    let mut current = arrange(natural.to_vec(), order, K::clone);
    let Some(at) = current.iter().position(|entry| entry == key) else {
        return;
    };

    let to = if down { at + 1 } else { at.wrapping_sub(1) };
    if to >= current.len() {
        // Already at the end it was heading for; nothing to swap with.
        return;
    }
    current.swap(at, to);
    *order = current;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn an_arrangement_survives_the_round_trip() {
        let arrangement = Arrangement {
            projects: keys(&["/home/pp/api", "/home/pp/dotfiles"]),
            rows: BTreeMap::from([(
                "/home/pp/api".to_owned(),
                vec![("main".to_owned(), 3), ("other".to_owned(), 7)],
            )]),
            folded: BTreeSet::from(["/home/pp/dotfiles".to_owned()]),
            names: BTreeMap::from([
                ("/home/pp/acme-frontend".to_owned(), "acme".to_owned()),
                ("/home/pp/acme-backend".to_owned(), "acme".to_owned()),
            ]),
            hosts: vec!["dev1".to_owned(), "build2".to_owned()],
        };

        assert_eq!(decode(&encode(&arrangement)), arrangement);
    }

    #[test]
    fn a_file_that_is_half_written_costs_only_what_it_lost() {
        let text = "p\t/home/pp/api\nnonsense\nr\t/home/pp/api\tmain\tnot-a-pane\n\
                    f\t\nr\t/home/pp/api\tmain\t4\nf\t/home/pp/api\n";
        let arrangement = decode(text);

        assert_eq!(arrangement.projects, keys(&["/home/pp/api"]));
        assert_eq!(
            arrangement.rows["/home/pp/api"],
            vec![("main".to_owned(), 4)]
        );
        assert_eq!(
            arrangement.folded,
            BTreeSet::from(["/home/pp/api".to_owned()])
        );
    }

    #[test]
    fn nothing_remembered_changes_nothing() {
        let items = keys(&["a", "b", "c"]);
        assert_eq!(arrange(items.clone(), &[], String::clone), items);
    }

    #[test]
    fn remembered_first_then_the_rest_as_they_came() {
        let items = keys(&["a", "b", "c", "d"]);
        let order = keys(&["c", "a"]);

        assert_eq!(
            arrange(items, &order, String::clone),
            keys(&["c", "a", "b", "d"]),
            "b and d keep their order behind the two that were placed"
        );
    }

    #[test]
    fn an_entry_that_is_gone_leaves_no_hole() {
        let items = keys(&["a", "b"]);
        let order = keys(&["gone", "b"]);

        assert_eq!(arrange(items, &order, String::clone), keys(&["b", "a"]));
    }

    #[test]
    fn moving_starts_from_what_is_on_screen() {
        let natural = keys(&["a", "b", "c"]);
        let mut order = Vec::new();
        shift(&mut order, &natural, &"c".to_string(), false);

        assert_eq!(
            order,
            keys(&["a", "c", "b"]),
            "c moved up one, not to the top"
        );
    }

    #[test]
    fn moving_down_and_back_returns_the_list() {
        let natural = keys(&["a", "b", "c"]);
        let mut order = Vec::new();
        shift(&mut order, &natural, &"a".to_string(), true);
        assert_eq!(order, keys(&["b", "a", "c"]));

        shift(&mut order, &natural, &"a".to_string(), false);
        assert_eq!(order, keys(&["a", "b", "c"]));
    }

    #[test]
    fn the_ends_hold() {
        let natural = keys(&["a", "b"]);
        let mut order = keys(&["a", "b"]);

        shift(&mut order, &natural, &"a".to_string(), false);
        assert_eq!(order, keys(&["a", "b"]), "nothing above the top");

        shift(&mut order, &natural, &"b".to_string(), true);
        assert_eq!(order, keys(&["a", "b"]), "nothing below the bottom");
    }

    #[test]
    fn something_new_arrives_at_the_end_rather_than_reshuffling_the_rest() {
        let mut order = Vec::new();
        shift(&mut order, &keys(&["a", "b"]), &"b".to_string(), false);
        assert_eq!(order, keys(&["b", "a"]));

        // "c" turns up later and nobody has an opinion about it yet.
        assert_eq!(
            arrange(keys(&["a", "b", "c"]), &order, String::clone),
            keys(&["b", "a", "c"])
        );
    }
}
