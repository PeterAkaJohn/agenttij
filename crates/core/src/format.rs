//! Fitting agent rows into a narrow sidebar.

use crate::agent::Agent;

/// One sidebar row: `<glyph> <label>       <age>`, exactly `width` wide so the
/// age column lines up down the list.
///
/// Colour and selection are the plugin's business; this is only the layout.
/// `current_session` marks which rows are local: an agent elsewhere is prefixed
/// with `⇢`, because reaching it costs a detach and that should never be a
/// surprise.
pub fn row(agent: &Agent, now: u64, width: usize, current_session: &str) -> String {
    let age = age(now, agent.reported_at);
    let glyph = agent.status.glyph();
    let elsewhere = if agent.session == current_session {
        ""
    } else {
        "⇢"
    };

    // glyph, its space, the elsewhere marker, the gap before the age, and the
    // age itself.
    let reserved = 3 + elsewhere.chars().count() + age.chars().count();
    if width <= reserved {
        return truncate(&format!("{glyph} {elsewhere}{}", agent.label()), width);
    }

    let label_width = width - reserved;
    let label = truncate(agent.label(), label_width);
    let gap = " ".repeat(label_width - label.chars().count());

    format!("{glyph} {elsewhere}{label}{gap} {age}")
}

/// First row to show, so the cursor stays visible in a list taller than the
/// pane. Keeps the cursor on the last line while scrolling down.
pub fn scroll_offset(cursor: usize, capacity: usize) -> usize {
    if capacity == 0 || cursor < capacity {
        return 0;
    }
    cursor + 1 - capacity
}

/// Compact age, at most four characters wide so the column never wobbles.
pub fn age(now: u64, reported_at: u64) -> String {
    if reported_at == 0 {
        return "-".to_string();
    }

    let seconds = now.saturating_sub(reported_at);
    match seconds {
        0..=9 => "now".to_string(),
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

/// Truncates to a display width, marking the cut with `…` so a clipped name is
/// never mistaken for a short one.
pub fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    match width {
        0 => String::new(),
        1 => "…".to_string(),
        _ => text.chars().take(width - 1).chain(['…']).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Status;

    fn agent(status: Status, reported_at: u64, cwd: &str) -> Agent {
        Agent {
            session: "sess".into(),
            pane: 1,
            status,
            reported_at,
            cwd: cwd.into(),
        }
    }

    #[test]
    fn a_row_is_glyph_label_and_right_aligned_age() {
        let agent = agent(Status::Running, 900, "/home/pp/api");
        assert_eq!(row(&agent, 1_020, 20, "sess"), "◐ api             2m");
    }

    #[test]
    fn every_row_fits_its_width_exactly() {
        let cases = [
            agent(Status::Running, 900, "/home/pp/api"),
            agent(
                Status::NeedsInput,
                1_019,
                "/home/pp/a-very-long-project-name",
            ),
            agent(Status::Unknown, 0, ""),
        ];

        for agent in &cases {
            for width in 1..40 {
                let row = row(agent, 1_020, width, "sess");
                let rendered = row.chars().count();
                if width > 3 + age(1_020, agent.reported_at).chars().count() {
                    assert_eq!(rendered, width, "{row:?} at width {width}");
                } else {
                    assert!(rendered <= width, "{row:?} overflows width {width}");
                }
            }
        }
    }

    #[test]
    fn a_row_never_wraps_in_a_pane_too_narrow_for_the_age() {
        let agent = agent(Status::Done, 1_000, "/home/pp/agenttij");
        assert_eq!(row(&agent, 1_020, 4, "sess"), "✓ a…");
    }

    #[test]
    fn scrolling_holds_the_cursor_on_screen() {
        assert_eq!(scroll_offset(0, 5), 0);
        assert_eq!(scroll_offset(4, 5), 0);
        assert_eq!(scroll_offset(5, 5), 1);
        assert_eq!(scroll_offset(9, 5), 5);
    }

    #[test]
    fn scrolling_a_pane_with_no_room_does_not_panic() {
        assert_eq!(scroll_offset(3, 0), 0);
    }

    #[test]
    fn ages_stay_short() {
        assert_eq!(age(1_000, 1_000), "now");
        assert_eq!(age(1_000, 970), "30s");
        assert_eq!(age(1_000, 400), "10m");
        assert_eq!(age(50_000, 1_000), "13h");
        assert_eq!(age(1_000_000, 1_000), "11d");

        for (now, reported) in [
            (1_000u64, 1_000u64),
            (1_000, 970),
            (1_000, 400),
            (10_000_000, 1),
        ] {
            assert!(age(now, reported).len() <= 4, "{now}/{reported} too wide");
        }
    }

    #[test]
    fn unknown_age_is_a_dash() {
        assert_eq!(age(1_000, 0), "-");
    }

    #[test]
    fn a_clock_that_went_backwards_does_not_panic() {
        assert_eq!(age(500, 1_000), "now");
    }

    #[test]
    fn truncation_marks_the_cut() {
        assert_eq!(truncate("agenttij", 20), "agenttij");
        assert_eq!(truncate("agenttij", 8), "agenttij");
        assert_eq!(truncate("agenttij", 5), "agen…");
        assert_eq!(truncate("agenttij", 1), "…");
        assert_eq!(truncate("agenttij", 0), "");
    }

    #[test]
    fn truncation_counts_characters_not_bytes() {
        assert_eq!(truncate("wörk-tréé", 5).chars().count(), 5);
    }
}
