//! Keeping the list in the order you put it in.
//!
//! The sidebar sorts by attention, which is right until you have an opinion.
//! Once you move something, that opinion wins for everything you have moved and
//! the sort keeps arranging the rest — so a project you dragged to the top stays
//! at the top, and an agent that appears later still lands somewhere sensible
//! instead of at a position nobody chose.

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
