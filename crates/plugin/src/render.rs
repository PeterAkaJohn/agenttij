//! Drawing the sidebar.
//!
//! Raw ANSI rather than Zellij's `Text` component. The component can only colour
//! text with one of the theme's four slots, which made every status look alike;
//! writing the escapes directly lets a status be any colour the user asked for.
//! Layout arithmetic stays in `agenttij_core::format`, where it is tested.

use agenttij_core::{format, Agent, Colors};

/// Reset everything: colour, and the weight the cursor row is drawn in.
const RESET: &str = "\u{1b}[0m";
/// The cursor row is bold rather than reversed. A block of inverted video across
/// a 20-column sidebar is the loudest thing on the screen, and it says the one
/// thing it should not: that the row it covers is the row you are looking at.
const CURSOR: &str = "\u{1b}[1m";

/// The first column says what a row is to you: where the keyboard is pointing,
/// and which row is actually on screen. They are the same until you open a pane
/// somewhere else, and then they are not — which is the whole reason to draw
/// them apart.
const AT_CURSOR: char = '›';
const ON_SCREEN: char = '▪';

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
    /// The pane holding the workspace slot, and the row that owns it — the thing
    /// you are looking at, which is not always the thing the cursor is on.
    pub on_screen: Option<u32>,
    pub on_screen_row: Option<u32>,
    /// Asked before something irreversible; takes the hint line's place.
    pub prompt: Option<&'a str>,
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
    // A question always gets its line, even when the list would rather have it:
    // a confirmation you cannot see is worse than a row you cannot.
    let hint_fits = view.agents.len() < view.rows && view.cols >= format::RAIL_MAX_COLS;
    let footer = view.rows > 0 && (view.prompt.is_some() || hint_fits);
    let capacity = if footer {
        view.rows.saturating_sub(1)
    } else {
        view.rows
    };

    let offset = format::scroll_offset(view.cursor, capacity);
    for (row, agent) in view.agents.iter().skip(offset).take(capacity).enumerate() {
        // The mark takes a column, so the row itself is drawn one narrower and
        // the ages still line up down the list.
        let (glyph, rest) = format::row_parts(
            agent,
            view.now,
            view.cols.saturating_sub(1),
            view.current_session,
        );
        let color = view.colors.of(agent.status);
        let selected = offset + row == view.cursor;
        let here = agent.session == view.current_session
            && (Some(agent.pane) == view.on_screen || Some(agent.pane) == view.on_screen_row);

        let mark = match (selected, here) {
            // Both, and the cursor is the more useful thing to say.
            (true, _) => AT_CURSOR,
            (false, true) => ON_SCREEN,
            _ => ' ',
        };

        at(row);
        if selected {
            print!("{CURSOR}");
        }
        print!("{mark}\u{1b}[{color}m{glyph}{RESET}");
        if selected {
            print!("{CURSOR}");
        }
        print!("{rest}{RESET}");
    }

    if footer {
        let hint = view.prompt.unwrap_or("j/k ↵ n a v b d  p peek");
        line(hint, view.rows - 1, view.cols);
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

/// One line of counts and whoever most wants you, in that agent's colour.
pub fn draw_bar(agents: &[Agent], now: u64, cols: usize, colors: &Colors) {
    let line = format::bar(agents, now, cols);
    let worst = agents
        .iter()
        .min_by_key(|agent| agent.status)
        .map(|agent| agent.status);
    let color = worst.map(|status| colors.of(status)).unwrap_or("0");
    let padding = cols.saturating_sub(line.chars().count());

    at(0);
    print!("\u{1b}[{color}m{line}{RESET}{}", " ".repeat(padding));
}

/// One plain line of a floating list, padded so the frame underneath is covered.
pub fn line_at(content: &str, row: usize, cols: usize) {
    line(content, row, cols);
}

/// One entry of the jump list: a mark, then the entry in its status colour.
pub fn entry_at(content: &str, row: usize, cols: usize, selected: bool, color: &str) {
    let mark = if selected { '›' } else { ' ' };
    let padding = cols.saturating_sub(1 + content.chars().count());
    at(row);
    if selected {
        print!("{CURSOR}");
    }
    print!("{mark}\u{1b}[{color}m{content}{RESET}");
    print!("{}", " ".repeat(padding));
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
