//! A status for an agent no hook reports on, read off its screen.
//!
//! Comparing two reads is what tells working from waiting, whatever the tool: a
//! screen that keeps changing is working, one that went still after working has
//! finished its turn, and a question at the bottom is waiting on you.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::agent::Status;

/// What the last read of a pane said, kept to compare the next one with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reading {
    pub fingerprint: u64,
    pub status: Status,
    /// When `status` last changed, standing in for the time of a report.
    pub since: u64,
}

/// What a waiting agent tends to end its screen with.
///
/// ponytail: a list of phrases rather than one per tool. Add a tool's own when
/// it asks some other way.
const QUESTIONS: [&str; 8] = [
    "(y/n)",
    "[y/n]",
    "(yes/no)",
    "do you want",
    "allow ",
    "approve",
    "press enter",
    "continue?",
];

/// Only the last few lines: a question answered a while ago is still on
/// screen further up.
const TAIL_LINES: usize = 4;

/// Reads `screen`, given what the read before it said.
pub fn read(before: Option<&Reading>, screen: &str, now: u64) -> Reading {
    let mut hasher = DefaultHasher::new();
    screen.hash(&mut hasher);
    let fingerprint = hasher.finish();

    let lines: Vec<&str> = screen
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let tail = lines[lines.len().saturating_sub(TAIL_LINES)..]
        .join("\n")
        .to_lowercase();
    let changed = before.is_some_and(|before| before.fingerprint != fingerprint);

    let status = if QUESTIONS.iter().any(|question| tail.contains(question)) {
        Status::NeedsInput
    } else {
        match before {
            // One read says nothing about whether anything is happening.
            None => Status::Unknown,
            Some(_) if changed => Status::Running,
            // Still after working: it finished its turn.
            Some(before) if before.status == Status::Running => Status::Done,
            Some(before) if before.status == Status::Unknown => Status::Idle,
            Some(before) => before.status,
        }
    };
    let since = match before {
        Some(before) if before.status == status => before.since,
        _ => now,
    };
    Reading {
        fingerprint,
        status,
        since,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn working_then_still_is_a_finished_turn() {
        let first = read(None, "$ codex\n", 10);
        assert_eq!(first.status, Status::Unknown, "one read says nothing");

        let quiet = read(Some(&first), "$ codex\n", 12);
        assert_eq!((quiet.status, quiet.since), (Status::Idle, 12));

        let working = read(Some(&quiet), "$ codex\nthinking ⠋\n", 14);
        assert_eq!((working.status, working.since), (Status::Running, 14));
        let still_working = read(Some(&working), "$ codex\nthinking ⠙\n", 16);
        assert_eq!(
            (still_working.status, still_working.since),
            (Status::Running, 14)
        );

        let done = read(Some(&still_working), "$ codex\nthinking ⠙\n", 18);
        assert_eq!((done.status, done.since), (Status::Done, 18));
        let still_done = read(Some(&done), "$ codex\nthinking ⠙\n", 20);
        assert_eq!((still_done.status, still_done.since), (Status::Done, 18));
    }

    #[test]
    fn a_question_at_the_bottom_is_waiting_on_you() {
        let asked = read(None, "edit src/main.rs\nApply this change? (y/n) ", 5);
        assert_eq!(asked.status, Status::NeedsInput);

        // Answered and scrolled on: what is at the bottom now is work.
        let mut screen = String::from("Apply this change? (y/n) y\n");
        for line in 0..6 {
            screen.push_str(&format!("writing {line}\n"));
        }
        assert_eq!(read(Some(&asked), &screen, 7).status, Status::Running);
    }
}
