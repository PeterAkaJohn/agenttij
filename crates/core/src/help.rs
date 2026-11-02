//! What the help pane lists.
//!
//! Here rather than in the renderer so the keys have one home: the sidebar reads
//! this, and a test checks it against nothing being forgotten.

/// A key and what it does, in the order they are worth learning.
pub const SIDEBAR_KEYS: [(&str, &str); 28] = [
    ("j / k", "move"),
    ("Enter", "show this group"),
    ("Tab", "open a group, or fold a project"),
    ("[ ]", "previous / next project"),
    ("J K", "move this project or group"),
    ("r", "name a project - same name joins them"),
    ("S", "rename this session"),
    ("h", "machines to watch, over ssh"),
    ("b", "flip to the previous group"),
    ("B", "back to the session you came from"),
    ("o", "a workspace here on that project"),
    ("v / V", "cycle panes in this group, either way"),
    ("1 - 9", "straight to that pane of the group"),
    ("'", "flip to the pane before this one"),
    ("a", "add a pane to this group"),
    ("m", "move this into the group on screen"),
    ("n", "new group (new agent)"),
    ("G", "new group in a directory you pick"),
    ("w", "new group in a worktree of its repo"),
    ("s", "share this session on the web (Zellij's screen)"),
    ("R", "answer a waiting agent (with reply \"true\")"),
    ("/", "jump: everywhere you could go"),
    ("p", "peek without leaving"),
    ("d d", "close this group, or this pane"),
    ("c c", "interrupt it - Ctrl-C, without going there"),
    ("!", "only what needs you"),
    ("q / Esc", "dismiss a peek"),
    ("?", "this list"),
];

/// The Zellij-level bindings the installer writes. Shown with their defaults;
/// a user who rebound them will see the default, which is the one honest
/// limitation of listing them here at all.
pub const GLOBAL_KEYS: [(&str, &str); 18] = [
    ("Alt ?", "this list, from any pane"),
    ("Alt s", "focus the sidebar"),
    ("Alt a", "a sidebar in any session, floating"),
    ("Alt v", "cycle panes in the group on screen"),
    ("Alt V", "and back the other way"),
    ("Alt 1-9", "straight to that pane of the group"),
    ("Alt '", "flip to the pane before this one"),
    ("Alt b", "flip to the previous group"),
    ("Alt g", "new group"),
    ("Alt G", "new group in a directory you pick"),
    ("Alt w", "new group in a git worktree"),
    ("Alt m", "add a pane to the group on screen"),
    ("Alt t", "jump - type `restore` for a workspace"),
    ("Alt ]", "fold the sidebar to a rail"),
    ("Alt [", "unfold it"),
    // Zellij's own, and the ones these features lean on.
    ("Ctrl o s", "share this session on the web (Zellij)"),
    ("Ctrl o ]", "keys back from a remote session (Zellij)"),
    ("Ctrl o f", "a remote session full screen (Zellij)"),
];

/// A column holds an entry at this width; two side by side need twice that.
const COLUMN: usize = 46;

/// The help pane's lines, wrapped to a width: the sidebar's keys and the
/// ones that work from any pane side by side where both columns fit, one
/// above the other where they do not.
pub fn lines(width: usize) -> Vec<String> {
    let section = |title: &str, keys: &[(&str, &str)], width: usize| {
        let mut out = vec![crate::format::truncate(title, width), String::new()];
        out.extend(keys.iter().map(|(key, what)| entry(key, what, width)));
        out
    };
    let gap = 2;
    if width >= 2 * COLUMN + gap {
        let column = (width - gap) / 2;
        let left = section("in the sidebar", &SIDEBAR_KEYS, column);
        let right = section("anywhere", &GLOBAL_KEYS, column);
        return (0..left.len().max(right.len()))
            .map(|row| {
                let left = left.get(row).map(String::as_str).unwrap_or_default();
                let right = right.get(row).map(String::as_str).unwrap_or_default();
                let padding = column + gap - left.chars().count();
                format!("{left}{}{right}", " ".repeat(padding))
                    .trim_end()
                    .to_owned()
            })
            .collect();
    }
    let mut out = section("in the sidebar", &SIDEBAR_KEYS, width);
    out.push(String::new());
    out.extend(section("anywhere", &GLOBAL_KEYS, width));
    out
}

fn entry(key: &str, what: &str, width: usize) -> String {
    // The keys column is fixed so the descriptions line up under each other.
    let column: usize = 9;
    let padding = column.saturating_sub(key.chars().count());
    let line = format!("{key}{}{what}", " ".repeat(padding));
    crate::format::truncate(&line, width)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_is_described() {
        for (key, what) in SIDEBAR_KEYS.iter().chain(GLOBAL_KEYS.iter()) {
            assert!(!key.is_empty() && !what.is_empty(), "{key:?} {what:?}");
        }
    }

    #[test]
    fn lines_fit_the_width_they_are_given() {
        for width in [10, 24, 40, 80] {
            for line in lines(width) {
                assert!(line.chars().count() <= width, "{line:?} at {width}");
            }
        }
    }

    #[test]
    fn both_sections_are_there() {
        for width in [40, 120] {
            let text = lines(width).join("\n");
            assert!(text.contains("in the sidebar") && text.contains("anywhere"));
            assert!(text.contains("Tab") && text.contains("Alt ]") && text.contains("Alt w"));
        }
    }

    #[test]
    fn a_wide_pane_puts_them_side_by_side() {
        let wide = lines(120);
        assert_eq!(wide.len(), SIDEBAR_KEYS.len().max(GLOBAL_KEYS.len()) + 2);
        assert!(wide[0].starts_with("in the sidebar") && wide[0].ends_with("anywhere"));
        assert!(wide.iter().all(|line| line.chars().count() <= 120));
        // Narrow, one above the other.
        assert_eq!(lines(60).len(), SIDEBAR_KEYS.len() + GLOBAL_KEYS.len() + 5);
    }
}
