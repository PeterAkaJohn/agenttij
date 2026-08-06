//! Drawing the sidebar.
//!
//! Layout arithmetic lives in `agenttij_core::format` where it is tested; this
//! module only adds colour, selection and placement.

use agenttij_core::{format, Agent};
use zellij_tile::prelude::*;

pub struct View<'a> {
    pub rows: usize,
    pub cols: usize,
    pub agents: &'a [Agent],
    pub cursor: usize,
    /// Host clock from the last scan, so ages match the clock that wrote them.
    pub now: u64,
    /// Shown instead of the list when there is nothing to show, or nothing we
    /// are allowed to show.
    pub notice: Option<&'a str>,
    /// Which session we are in, so rows elsewhere can be marked.
    pub current_session: &'a str,
}

pub fn draw(view: &View) {
    if let Some(notice) = view.notice {
        line(notice, 0, view.cols);
        return;
    }

    if view.agents.is_empty() {
        line("no agents", 0, view.cols);
        return;
    }

    // Leave the bottom line for the key hint, but only if the list does not
    // need it.
    let hint_fits = view.agents.len() < view.rows;
    let capacity = if hint_fits {
        view.rows.saturating_sub(1)
    } else {
        view.rows
    };

    let offset = format::scroll_offset(view.cursor, capacity);
    for (row, agent) in view.agents.iter().skip(offset).take(capacity).enumerate() {
        let mut text = Text::new(format::row(
            agent,
            view.now,
            view.cols,
            view.current_session,
        ))
        .color_range(agent.status.color_slot(), 0..1);
        if offset + row == view.cursor {
            text = text.selected();
        }
        print_text_with_coordinates(text, 0, row, Some(view.cols), Some(1));
    }

    if hint_fits {
        line("j/k ↵ go  p peek  c park", view.rows - 1, view.cols);
    }
}

fn line(content: &str, row: usize, cols: usize) {
    let text = Text::new(format::truncate(content, cols));
    print_text_with_coordinates(text, 0, row, Some(cols), Some(1));
}
