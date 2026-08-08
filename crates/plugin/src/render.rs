//! Drawing the sidebar.
//!
//! Raw ANSI rather than Zellij's `Text` component. The component can only colour
//! text with one of the theme's four slots, which made every status look alike;
//! writing the escapes directly lets a status be any colour the user asked for.
//! Layout arithmetic stays in `agenttij_core::format`, where it is tested.

use agenttij_core::{format, Agent, Colors};

/// Reset everything: colour, and the reverse video used for the cursor row.
const RESET: &str = "\u{1b}[0m";
/// Reverse video marks the selected row, so the cursor is visible whatever
/// colours the user chose.
const SELECTED: &str = "\u{1b}[7m";

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
    pub colors: &'a Colors,
}

pub fn draw(view: &View) {
    // Rows are padded to the full width, so the previous frame is overwritten
    // without a clear — clearing first makes the sidebar flicker every tick.
    if let Some(notice) = view.notice {
        line(notice, 0, view.cols);
        return;
    }

    if view.agents.is_empty() {
        line("no agents", 0, view.cols);
        return;
    }

    // Leave the bottom line for the key hint, but only if the list does not
    // need it and there is width to read it in.
    let hint_fits = view.agents.len() < view.rows && view.cols >= format::RAIL_MAX_COLS;
    let capacity = if hint_fits {
        view.rows.saturating_sub(1)
    } else {
        view.rows
    };

    let offset = format::scroll_offset(view.cursor, capacity);
    for (row, agent) in view.agents.iter().skip(offset).take(capacity).enumerate() {
        let (glyph, rest) = format::row_parts(agent, view.now, view.cols, view.current_session);
        let color = view.colors.of(agent.status);
        let selected = offset + row == view.cursor;

        at(row);
        if selected {
            print!("{SELECTED}");
        }
        print!("\u{1b}[{color}m{glyph}{RESET}");
        if selected {
            print!("{SELECTED}");
        }
        print!("{rest}{RESET}");
    }

    if hint_fits {
        line("j/k ↵ n a v b  p peek", view.rows - 1, view.cols);
    }
}

/// Moves to the start of a row, which is where every line begins.
fn at(row: usize) {
    print!("\u{1b}[{};1H", row + 1);
}

fn line(content: &str, row: usize, cols: usize) {
    let text = format::truncate(content, cols);
    let padding = cols.saturating_sub(text.chars().count());
    at(row);
    print!("{text}{}{RESET}", " ".repeat(padding));
}

/// Draws lines into a floating pane — a peek's mirrored screen, or the keybind
/// list — with a hint that any key closes it.
pub fn draw_peek(lines: &[String], rows: usize, cols: usize) {
    if lines.is_empty() {
        line("waiting for the pane…", 0, cols);
        return;
    }

    // Keep the last row for the hint: a pane you cannot tell how to close is
    // worse than no pane.
    let capacity = rows.saturating_sub(1);
    for (row, content) in lines.iter().take(capacity).enumerate() {
        let text = format::truncate(content, cols);
        let padding = cols.saturating_sub(text.chars().count());
        at(row);
        print!("{text}{}", " ".repeat(padding));
    }

    if rows > 0 {
        line("any key closes", rows - 1, cols);
    }
}
