//! UI-owned, credential-free state and display ordering. No GTK or file I/O.
mod activity;
pub use activity::{Activity, Operation};
use hangar_core::summary::{AccountSummary, QuotaSummary};
use std::cmp::Reverse;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuotaState {
    Unknown,
    Fresh,
    Old,
    Loading,
    Failed,
}

#[derive(Clone, Debug)]
pub struct Card {
    pub account: AccountSummary,
    pub quota: Option<QuotaSummary>,
    pub fetched_at: Option<u64>,
    pub state: QuotaState,
    pub recommended: bool,
}

impl Card {
    pub fn remaining(&self) -> Option<i32> {
        self.quota.as_ref().and_then(|q| q.weekly_remaining)
    }
}

#[derive(Default)]
pub struct Model {
    pub generation: u64,
    pub activity: Option<Activity>,
    pub cards: Vec<Card>,
}

impl Model {
    pub fn busy(&self) -> bool {
        self.activity.is_some()
    }

    pub fn progress(&mut self, generation: u64, completed: usize, total: usize) -> bool {
        generation == self.generation
            && self
                .activity
                .as_mut()
                .is_some_and(|a| a.set_progress(completed, total))
    }

    pub fn begin(&mut self, operation: Operation) -> Option<u64> {
        if self.busy() {
            return None;
        }
        self.generation += 1;
        self.activity = Some(Activity::new(operation));
        Some(self.generation)
    }

    pub fn apply(&mut self, generation: u64, mut cards: Vec<Card>, terminal: bool) -> bool {
        if generation != self.generation {
            return false;
        }
        if terminal || self.cards.is_empty() {
            sort_cards(&mut cards);
        } else {
            // Only the projection moves. Never reorder the persisted accounts.
            cards.sort_by_key(|card| {
                self.cards
                    .iter()
                    .position(|old| old.account.id == card.account.id)
                    .unwrap_or(usize::MAX)
            });
        }
        self.cards = cards;
        true
    }
}

fn sort_cards(cards: &mut [Card]) {
    cards.sort_by_key(|card| {
        let group = if card.account.current {
            0
        } else if card.account.stale {
            4
        } else if card.recommended {
            1
        } else if card.remaining().is_some() {
            2
        } else {
            3
        };
        (group, Reverse(card.remaining().unwrap_or(-1)))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use hangar_core::summary::{AccessTokenSummaryState, TokenHealthSummary};

    fn card(id: &str, remaining: Option<i32>) -> Card {
        Card {
            account: AccountSummary {
                id: id.into(),
                email: format!("{id}@example.com"),
                current: false,
                stale: false,
                account_id: None,
                organization_id: None,
                expires_at: None,
                token_health: TokenHealthSummary {
                    status: AccessTokenSummaryState::Unknown,
                    expiry_source: None,
                    refresh_available: false,
                },
            },
            quota: Some(QuotaSummary {
                plan: None,
                weekly_remaining: remaining,
                weekly_reset_at: None,
                reset_available: None,
                reset_next_expiry: None,
            }),
            fetched_at: None,
            state: QuotaState::Unknown,
            recommended: false,
        }
    }

    #[test]
    fn current_even_stale_then_recommended_known_unknown_stale() {
        let mut cards = vec![
            card("unknown", None),
            card("high", Some(90)),
            card("recommended", Some(80)),
            card("current", Some(2)),
            card("stale", Some(99)),
        ];
        cards[2].recommended = true;
        cards[3].account.current = true;
        cards[3].account.stale = true;
        cards[4].account.stale = true;
        sort_cards(&mut cards);
        assert_eq!(
            cards
                .iter()
                .map(|c| c.account.id.as_str())
                .collect::<Vec<_>>(),
            ["current", "recommended", "high", "unknown", "stale"]
        );
    }

    #[test]
    fn refresh_retains_positions_until_terminal_and_equal_values_keep_library_order() {
        let mut model = Model::default();
        let generation = model.begin(Operation::Refresh).unwrap();
        model.apply(
            generation,
            vec![card("a", Some(80)), card("b", Some(40))],
            false,
        );
        model.apply(
            generation,
            vec![card("a", Some(80)), card("b", Some(95))],
            false,
        );
        assert_eq!(model.cards[0].account.id, "a");
        model.apply(
            generation,
            vec![card("a", Some(80)), card("b", Some(95))],
            true,
        );
        assert_eq!(model.cards[0].account.id, "b");
        model.apply(
            generation,
            vec![card("a", Some(80)), card("b", Some(80))],
            true,
        );
        assert_eq!(model.cards[0].account.id, "a");
    }

    #[test]
    fn progress_after_completion_or_from_an_old_task_cannot_restore_feedback() {
        let mut model = Model::default();
        let old = model.begin(Operation::Refresh).unwrap();
        assert!(model.progress(old, 1, 3));
        model.activity = None;
        assert!(!model.progress(old, 2, 3));
        assert!(!model.busy());
        let current = model.begin(Operation::Refresh).unwrap();
        assert!(!model.progress(old, 3, 3));
        assert!(model.progress(current, 0, 2));
        assert!(model.activity.as_ref().unwrap().message().contains("0/2"));
    }

    #[test]
    fn late_generation_cannot_replace_state_and_busy_blocks_duplicate_operations() {
        let mut model = Model::default();
        model.begin(Operation::Refresh);
        assert_eq!(model.begin(Operation::Refresh), None);
        assert!(!model.apply(0, vec![card("late", Some(100))], true));
        assert!(model.cards.is_empty());
    }
}
