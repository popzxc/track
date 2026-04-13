use std::{
    collections::BTreeSet,
    sync::{Condvar, Mutex, OnceLock},
};

#[derive(Debug, Default)]
struct ReviewDispatchStartGate {
    active_review_ids: Mutex<BTreeSet<String>>,
    wake_waiters: Condvar,
}

#[derive(Debug)]
pub(crate) struct ReviewDispatchStartGuard {
    review_id: String,
}

impl ReviewDispatchStartGuard {
    pub(crate) fn acquire(review_id: &str) -> Self {
        let gate = review_dispatch_start_gate();
        let mut active_review_ids = gate
            .active_review_ids
            .lock()
            .expect("review dispatch start gate should not be poisoned");

        while active_review_ids.contains(review_id) {
            active_review_ids = gate
                .wake_waiters
                .wait(active_review_ids)
                .expect("review dispatch start gate should not be poisoned");
        }

        active_review_ids.insert(review_id.to_owned());

        Self {
            review_id: review_id.to_owned(),
        }
    }
}

impl Drop for ReviewDispatchStartGuard {
    fn drop(&mut self) {
        let gate = review_dispatch_start_gate();
        let mut active_review_ids = gate
            .active_review_ids
            .lock()
            .expect("review dispatch start gate should not be poisoned");
        active_review_ids.remove(&self.review_id);
        gate.wake_waiters.notify_all();
    }
}

fn review_dispatch_start_gate() -> &'static ReviewDispatchStartGate {
    static GATE: OnceLock<ReviewDispatchStartGate> = OnceLock::new();

    // Reviews are now follow-up capable, so the same "check for active work,
    // then persist a preparing record" race that tasks already guard against
    // applies here too. Keeping a dedicated gate per review id preserves the
    // review domain boundary without forcing the task flow to share review-only
    // coordination state.
    GATE.get_or_init(ReviewDispatchStartGate::default)
}
