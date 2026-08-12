//! What the help pane lists.
//!
//! Here rather than in the renderer so the keys have one home: the sidebar reads
//! this, and a test checks it against nothing being forgotten.

/// A key and what it does, in the order they are worth learning.
pub const SIDEBAR_KEYS: [(&str, &str); 22] = [
    ("j / k", "move"),
    ("Enter", "show this row"),
    ("Tab", "open a row, or fold a project"),
    ("[ ]", "previous / next project"),
    ("J K", "move this project or row"),
    ("r", "name a project — same name joins them"),
    ("h", "machines to watch, over ssh"),
    ("b", "flip to the previous row"),
    ("B", "back to the session you came from"),
    ("o", "a workspace here on that project"),
    ("v / V", "cycle panes in this row, either way"),
    ("1 - 9", "straight to that pane of the row"),
    ("'", "flip to the pane before this one"),
    ("a", "add a pane to this row"),
    ("n", "new row (new agent)"),
    ("/", "jump: everywhere you could go"),
    ("p", "peek without leaving"),
    ("d d", "close this row, or this pane"),
    ("c c", "interrupt it — Ctrl-C, without going there"),
    ("!", "only what needs you"),
    ("q / Esc", "dismiss a peek"),
    ("?", "this list"),
];

/// The Zellij-level bindings the installer writes. Shown with their defaults;
/// a user who rebound them will see the default, which is the one honest
/// limitation of listing them here at all.
pub const GLOBAL_KEYS: [(&str, &str); 10] = [
    ("Alt s", "focus the sidebar"),
    ("Alt v", "cycle panes in this row"),
    ("Alt V", "and back the other way"),
    ("Alt 1-9", "straight to that pane of the row"),
    ("Alt '", "flip to the pane before this one"),
    ("Alt b", "previous row"),
    ("Alt g", "new row"),
    ("Alt m", "add a pane to this row"),
    ("Alt t", "jump, from anywhere"),
    ("Alt ]", "fold the sidebar to a rail"),
];

/// The help pane's lines, wrapped to a width.
pub fn lines(width: usize) -> Vec<String> {
    let heading = |text: &str| crate::format::truncate(text, width);

    let mut out = vec![heading("in the sidebar"), String::new()];
    out.extend(
        SIDEBAR_KEYS
            .iter()
            .map(|(key, what)| entry(key, what, width)),
    );
    out.push(String::new());
    out.push(heading("anywhere"));
    out.push(String::new());
    out.extend(
        GLOBAL_KEYS
            .iter()
            .map(|(key, what)| entry(key, what, width)),
    );
    out.push(String::new());
    out.push(heading("any key closes this"));
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
        let text = lines(40).join("\n");
        assert!(text.contains("in the sidebar") && text.contains("anywhere"));
        assert!(text.contains("Tab") && text.contains("Alt ]"));
    }
}
