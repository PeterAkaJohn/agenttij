//! A sidebar row is a group of panes: an agent, and the companions you keep
//! beside it.
//!
//! Exactly one member of a group is on screen at a time, so a row is a place to
//! work rather than a single pane — an agent with an editor and a log beside it
//! is one row, not three. Companions never get a row of their own.
//!
//! Every pane belongs to exactly one group. A pane the sidebar has not been told
//! about becomes a group of its own, which is what keeps a reload cheap: it costs
//! you the grouping, never access to a pane.

/// One row's worth of panes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// Members in the order they joined. The first is the *primary* — the agent
    /// whose status and name the row shows.
    pub members: Vec<u32>,
    /// The member last on screen, so returning to a row puts you back where you
    /// were rather than always on the agent.
    pub current: u32,
}

impl Group {
    fn new(pane: u32) -> Self {
        Self {
            members: vec![pane],
            current: pane,
        }
    }

    pub fn primary(&self) -> u32 {
        self.members[0]
    }

    pub fn holds(&self, pane: u32) -> bool {
        self.members.contains(&pane)
    }

    /// The member after `pane`, wrapping. `None` when there is nothing to cycle
    /// to, which is the common case of an agent on its own.
    fn after(&self, pane: u32) -> Option<u32> {
        if self.members.len() < 2 {
            return None;
        }
        let at = self.members.iter().position(|member| *member == pane)?;
        Some(self.members[(at + 1) % self.members.len()])
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Groups {
    groups: Vec<Group>,
    /// Members asked for but not yet seen alive, with the number of updates
    /// they may still go unseen.
    ///
    /// A pane joins a row the moment it is asked for, and several updates can
    /// arrive before the one that proves it exists — a focus change alone
    /// triggers one, carrying a pane list from before the pane was made. Without
    /// this grace, reconciliation drops the new member and the row falls back to
    /// singletons. The count is a bound, so a pane that never appears cannot
    /// haunt a row forever.
    unseen: Vec<(u32, u8)>,
}

impl Groups {
    /// Brings the grouping in line with the panes that actually exist: dead
    /// members are dropped, empty groups disappear, and anything unrecognised
    /// becomes its own group.
    pub fn reconcile(&mut self, live: &[u32]) {
        // Anything seen alive is no longer waiting to be seen; anything still
        // waiting spends one of its lives.
        self.unseen.retain(|(pane, _)| !live.contains(pane));
        for (_, lives) in self.unseen.iter_mut() {
            *lives = lives.saturating_sub(1);
        }
        self.unseen.retain(|(_, lives)| *lives > 0);

        let waiting: Vec<u32> = self.unseen.iter().map(|(pane, _)| *pane).collect();
        for group in &mut self.groups {
            group
                .members
                .retain(|member| live.contains(member) || waiting.contains(member));
        }
        self.groups.retain(|group| !group.members.is_empty());

        for group in &mut self.groups {
            if !group.members.contains(&group.current) {
                group.current = group.primary();
            }
        }

        for pane in live {
            if !self.groups.iter().any(|group| group.holds(*pane)) {
                self.groups.push(Group::new(*pane));
            }
        }

        // Ordered by primary so rows do not shuffle between updates.
        self.groups.sort_by_key(Group::primary);
    }

    /// Adds a pane to whichever group holds `beside`, and shows it.
    pub fn add(&mut self, beside: u32, pane: u32) {
        match self.groups.iter_mut().find(|group| group.holds(beside)) {
            Some(group) => {
                if !group.holds(pane) {
                    group.members.push(pane);
                }
                group.current = pane;
            }
            None => self.groups.push(Group::new(pane)),
        }
        // Ten updates is a second or so of slack — far more than the two or
        // three that arrive around a pane being created.
        self.unseen.push((pane, 10));
    }

    /// The next member to cycle to within the group holding `pane`.
    pub fn next_after(&self, pane: u32) -> Option<u32> {
        self.group_of(pane)?.after(pane)
    }

    /// Records which member of a group is on screen.
    pub fn show(&mut self, pane: u32) {
        if let Some(group) = self.groups.iter_mut().find(|group| group.holds(pane)) {
            group.current = pane;
        }
    }

    pub fn group_of(&self, pane: u32) -> Option<&Group> {
        self.groups.iter().find(|group| group.holds(pane))
    }

    /// The member of `pane`'s group that should be on screen.
    pub fn current_of(&self, pane: u32) -> Option<u32> {
        self.group_of(pane).map(|group| group.current)
    }

    /// The panes a row owns, in the order they joined.
    pub fn members_of(&self, primary: u32) -> &[u32] {
        self.group_of(primary)
            .map(|group| group.members.as_slice())
            .unwrap_or(&[])
    }

    /// One entry per row: the primary, and how many panes the row owns.
    pub fn rows(&self) -> impl Iterator<Item = (u32, usize)> + '_ {
        self.groups
            .iter()
            .map(|group| (group.primary(), group.members.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_panes_become_their_own_rows() {
        let mut groups = Groups::default();
        groups.reconcile(&[3, 4]);

        assert_eq!(groups.rows().collect::<Vec<_>>(), vec![(3, 1), (4, 1)]);
    }

    #[test]
    fn a_companion_does_not_get_its_own_row() {
        let mut groups = Groups::default();
        groups.reconcile(&[3]);
        groups.add(3, 4);
        groups.reconcile(&[3, 4]);

        assert_eq!(groups.rows().collect::<Vec<_>>(), vec![(3, 2)]);
        assert_eq!(groups.current_of(3), Some(4), "the new pane is showing");
    }

    #[test]
    fn a_rows_members_come_back_in_the_order_they_joined() {
        let mut groups = Groups::default();
        groups.reconcile(&[3]);
        groups.add(3, 5);
        groups.add(3, 4);

        assert_eq!(groups.members_of(3), &[3, 5, 4]);
        assert_eq!(groups.members_of(99), &[] as &[u32]);
    }

    /// The update that proves a new pane exists may not be the next one to
    /// arrive, so a just-added pane survives one reconciliation without proof.
    #[test]
    fn a_just_added_pane_survives_an_update_that_has_not_seen_it() {
        let mut groups = Groups::default();
        groups.reconcile(&[3]);
        groups.add(3, 4);

        groups.reconcile(&[3]); // stale: the new pane is not in this list yet
        assert_eq!(groups.rows().collect::<Vec<_>>(), vec![(3, 2)]);

        groups.reconcile(&[3, 4]); // and now it is
        assert_eq!(groups.rows().collect::<Vec<_>>(), vec![(3, 2)]);
    }

    /// It does not get a second reprieve: a pane that never appears is gone.
    #[test]
    fn a_pane_that_never_arrives_is_dropped_eventually() {
        let mut groups = Groups::default();
        groups.reconcile(&[3]);
        groups.add(3, 4);

        for _ in 0..9 {
            groups.reconcile(&[3]);
        }
        assert_eq!(
            groups.rows().collect::<Vec<_>>(),
            vec![(3, 2)],
            "still waiting"
        );

        groups.reconcile(&[3]);
        assert_eq!(
            groups.rows().collect::<Vec<_>>(),
            vec![(3, 1)],
            "given up on"
        );
    }

    /// Once a pane has been seen, it is held to the normal rule again.
    #[test]
    fn a_member_seen_once_is_dropped_as_soon_as_it_goes() {
        let mut groups = Groups::default();
        groups.reconcile(&[3]);
        groups.add(3, 4);

        groups.reconcile(&[3, 4]);
        groups.reconcile(&[3]);
        assert_eq!(groups.rows().collect::<Vec<_>>(), vec![(3, 1)]);
    }

    #[test]
    fn cycling_walks_the_group_and_wraps() {
        let mut groups = Groups::default();
        groups.reconcile(&[3]);
        groups.add(3, 4);
        groups.add(3, 5);

        assert_eq!(groups.next_after(3), Some(4));
        assert_eq!(groups.next_after(4), Some(5));
        assert_eq!(groups.next_after(5), Some(3));
    }

    #[test]
    fn an_agent_on_its_own_has_nowhere_to_cycle() {
        let mut groups = Groups::default();
        groups.reconcile(&[3]);

        assert_eq!(groups.next_after(3), None);
    }

    #[test]
    fn closing_a_companion_leaves_the_row_alone() {
        let mut groups = Groups::default();
        groups.reconcile(&[3]);
        groups.add(3, 4);
        groups.reconcile(&[3, 4]); // seen
        groups.reconcile(&[3]); // and now gone

        assert_eq!(groups.rows().collect::<Vec<_>>(), vec![(3, 1)]);
        assert_eq!(
            groups.current_of(3),
            Some(3),
            "showing falls back to the agent"
        );
    }

    /// Losing the agent must not strand the panes that were beside it.
    #[test]
    fn closing_the_primary_promotes_the_next_member() {
        let mut groups = Groups::default();
        groups.reconcile(&[3]);
        groups.add(3, 4);
        groups.reconcile(&[3, 4]); // seen
        groups.reconcile(&[4]); // the agent closes

        assert_eq!(groups.rows().collect::<Vec<_>>(), vec![(4, 1)]);
    }

    #[test]
    fn a_group_is_never_left_showing_a_dead_pane() {
        let mut groups = Groups::default();
        groups.reconcile(&[3]);
        groups.add(3, 4);
        assert_eq!(groups.current_of(3), Some(4));

        groups.reconcile(&[3, 4]); // seen
        groups.reconcile(&[3]); // and now gone
        assert_eq!(groups.current_of(3), Some(3));
    }

    #[test]
    fn rows_hold_their_order_however_panes_arrive() {
        let mut first = Groups::default();
        first.reconcile(&[7, 3, 5]);
        let mut second = Groups::default();
        second.reconcile(&[5, 7, 3]);

        assert_eq!(
            first.rows().collect::<Vec<_>>(),
            second.rows().collect::<Vec<_>>()
        );
    }
}
