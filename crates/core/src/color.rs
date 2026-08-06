//! Status colours, resolved to SGR parameters.
//!
//! The sidebar draws with raw ANSI rather than Zellij's `Text` component,
//! because that component can only colour text with one of the theme's four
//! slots — which is why every status looked alike. Here a colour is whatever the
//! user wrote, kept as the body of an SGR sequence (`\x1b[<body>m`).

use crate::agent::Status;
use std::collections::BTreeMap;

/// Obvious defaults: green finished, yellow waiting on you, blue working.
const DEFAULTS: [(Status, &str); 6] = [
    (Status::NeedsInput, "yellow"),
    (Status::Running, "blue"),
    (Status::Done, "green"),
    (Status::Idle, "bright-black"),
    (Status::Unknown, "magenta"),
    (Status::Pane, "bright-black"),
];

/// Parses a colour into SGR parameters for a foreground colour.
///
/// Accepts a name (`green`, `bright-black`), a 256-colour index (`0`–`255`), or
/// a hex triplet (`#ff8800`). Anything else is `None`, so a typo in a layout
/// falls back to the default rather than painting the row with a stray escape.
pub fn parse(spec: &str) -> Option<String> {
    let spec = spec.trim().to_lowercase();
    if spec.is_empty() {
        return None;
    }

    if let Some(hex) = spec.strip_prefix('#') {
        if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
        return Some(format!(
            "38;2;{};{};{}",
            channel(0)?,
            channel(2)?,
            channel(4)?
        ));
    }

    if let Ok(index) = spec.parse::<u8>() {
        return Some(format!("38;5;{index}"));
    }

    let (name, bright) = match spec.strip_prefix("bright-") {
        Some(rest) => (rest, true),
        None => (spec.as_str(), false),
    };
    let base = match name {
        "black" => 30,
        "red" => 31,
        "green" => 32,
        "yellow" => 33,
        "blue" => 34,
        "magenta" => 35,
        "cyan" => 36,
        "white" => 37,
        _ => return None,
    };
    Some(format!("{}", if bright { base + 60 } else { base }))
}

/// A colour per status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Colors {
    sgr: BTreeMap<Status, String>,
}

impl Default for Colors {
    fn default() -> Self {
        let sgr = DEFAULTS
            .iter()
            .filter_map(|(status, spec)| parse(spec).map(|sgr| (*status, sgr)))
            .collect();
        Self { sgr }
    }
}

impl Colors {
    /// Reads `<status>=<colour>` pairs, e.g. `done=green,needs-input=#ffcc00`.
    /// Unknown statuses and unparseable colours are ignored, leaving the default
    /// in place: a mistake in one entry must not blank the rest of the list.
    pub fn from_pairs(raw: &str) -> Self {
        let mut colors = Self::default();

        for pair in raw.split(',') {
            let Some((status, spec)) = pair.split_once('=') else {
                continue;
            };
            let Some(status) = status_named(status.trim()) else {
                continue;
            };
            if let Some(sgr) = parse(spec) {
                colors.sgr.insert(status, sgr);
            }
        }
        colors
    }

    /// SGR body for a status, e.g. `33`.
    pub fn of(&self, status: Status) -> &str {
        self.sgr.get(&status).map(String::as_str).unwrap_or("39")
    }
}

/// The names users write in configuration, matching the hook's state words.
fn status_named(name: &str) -> Option<Status> {
    match name.to_lowercase().as_str() {
        "needs-input" => Some(Status::NeedsInput),
        "running" => Some(Status::Running),
        "done" => Some(Status::Done),
        "idle" => Some(Status::Idle),
        "unknown" => Some(Status::Unknown),
        "pane" => Some(Status::Pane),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_become_basic_colours() {
        assert_eq!(parse("green").as_deref(), Some("32"));
        assert_eq!(parse(" Yellow ").as_deref(), Some("33"));
        assert_eq!(parse("bright-black").as_deref(), Some("90"));
    }

    #[test]
    fn indices_and_hex_are_accepted() {
        assert_eq!(parse("220").as_deref(), Some("38;5;220"));
        assert_eq!(parse("#ff8800").as_deref(), Some("38;2;255;136;0"));
        assert_eq!(parse("#FF8800").as_deref(), Some("38;2;255;136;0"));
    }

    #[test]
    fn nonsense_is_rejected_rather_than_emitted() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("chartreuse"), None);
        assert_eq!(parse("#ff88"), None);
        assert_eq!(parse("#gggggg"), None);
        assert_eq!(parse("300"), None); // not a u8
        assert_eq!(parse("31m\u{1b}[J"), None);
    }

    #[test]
    fn defaults_are_the_obvious_ones() {
        let colors = Colors::default();
        assert_eq!(colors.of(Status::Done), "32"); // green
        assert_eq!(colors.of(Status::NeedsInput), "33"); // yellow
        assert_eq!(colors.of(Status::Running), "34"); // blue
    }

    #[test]
    fn pairs_override_only_what_they_name() {
        let colors = Colors::from_pairs("done=#00ff00, needs-input=red");

        assert_eq!(colors.of(Status::Done), "38;2;0;255;0");
        assert_eq!(colors.of(Status::NeedsInput), "31");
        assert_eq!(colors.of(Status::Running), "34", "untouched");
    }

    #[test]
    fn a_bad_entry_leaves_the_rest_alone() {
        let colors = Colors::from_pairs("done=nonsense,bogus=red,running=cyan,malformed");

        assert_eq!(colors.of(Status::Done), "32", "kept the default");
        assert_eq!(colors.of(Status::Running), "36");
    }
}
