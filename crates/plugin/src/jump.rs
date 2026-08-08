//! The jump palette: a floating list of everywhere you could go, filtered by
//! typing.
//!
//! Another instance of this same plugin, like a peek and the keybind list, for
//! the same reason: a floating pane is only on screen while it holds focus, and
//! only a plugin pane can hold focus and read the keys being typed into it.

use agenttij_core::{jump, Colors};
use zellij_tile::prelude::*;

use crate::render;

/// What a keystroke asked for.
pub enum Act {
    /// Keep typing.
    Stay,
    /// Nothing more to do here.
    Close,
    /// Go to this entry, then close.
    Go(jump::Target),
}

#[derive(Default)]
pub struct Jump {
    typed: String,
    /// Everything, in the order the list was built.
    entries: Vec<jump::Entry>,
    /// Indices into `entries` that match what has been typed, best first.
    matching: Vec<usize>,
    cursor: usize,
}

impl Jump {
    /// Replaces the list without losing your place: what you are pointing at is
    /// remembered by what it is, not by where it was, because the list is
    /// rebuilt under you every second.
    pub fn refresh(&mut self, entries: Vec<jump::Entry>) {
        let was = self.selected().cloned();
        self.entries = entries;
        self.filter();

        if let Some(was) = was {
            if let Some(at) = self
                .matching
                .iter()
                .position(|at| self.entries[*at].target == was.target)
            {
                self.cursor = at;
            }
        }
    }

    pub fn key(&mut self, key: BareKey) -> Act {
        match key {
            BareKey::Esc => return Act::Close,
            BareKey::Enter => {
                return match self.selected() {
                    Some(entry) => Act::Go(entry.target.clone()),
                    None => Act::Close,
                }
            }
            BareKey::Down | BareKey::Tab => self.move_cursor(1),
            BareKey::Up => self.move_cursor(-1),
            BareKey::Backspace => {
                self.typed.pop();
                self.filter();
            }
            BareKey::Char(letter) => {
                self.typed.push(letter);
                self.filter();
            }
            _ => {}
        }
        Act::Stay
    }

    pub fn draw(&self, rows: usize, cols: usize, colors: &Colors) {
        if rows == 0 {
            return;
        }
        render::line_at(&format!("› {}▏", self.typed), 0, cols);

        // One line for what is being typed, one for the count at the bottom.
        let capacity = rows.saturating_sub(2);
        let offset = agenttij_core::format::scroll_offset(self.cursor, capacity);
        for (row, at) in self.matching.iter().skip(offset).take(capacity).enumerate() {
            let entry = &self.entries[*at];
            let selected = offset + row == self.cursor;
            render::entry_at(
                &jump::line(entry, cols.saturating_sub(1)),
                row + 1,
                cols,
                selected,
                colors.of(jump::status(entry)),
            );
        }

        let count = if self.matching.is_empty() {
            "nothing matches".to_owned()
        } else {
            format!(
                "{} of {}  ↵ go  esc close",
                self.matching.len(),
                self.entries.len()
            )
        };
        render::line_at(&count, rows - 1, cols);
    }

    fn selected(&self) -> Option<&jump::Entry> {
        self.matching.get(self.cursor).map(|at| &self.entries[*at])
    }

    /// Re-ranks and puts the cursor back on the best match, which is where
    /// someone who just typed a letter is looking.
    fn filter(&mut self) {
        self.matching = jump::rank(&self.entries, &self.typed);
        self.cursor = 0;
    }

    fn move_cursor(&mut self, delta: isize) {
        if self.matching.is_empty() {
            return;
        }
        let last = self.matching.len() - 1;
        // Wraps, because a list you can walk off the end of is a list you have to
        // look at while you walk it.
        self.cursor = match self.cursor.checked_add_signed(delta) {
            Some(at) if at <= last => at,
            Some(_) => 0,
            None => last,
        };
    }
}
