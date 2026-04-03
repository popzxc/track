use std::collections::BTreeSet;
use std::sync::{Condvar, Mutex, OnceLock};

#[derive(Debug, Default)]
struct TaskDispatchStartGate {
    active_task_ids: Mutex<BTreeSet<String>>,
    wake_waiters: Condvar,
}

#[derive(Debug)]
pub(crate) struct TaskDispatchStartGuard {
    task_id: String,
}

impl TaskDispatchStartGuard {
    pub(crate) fn acquire(task_id: &str) -> Self {
        let gate = task_dispatch_start_gate();
        let mut active_task_ids = gate
            .active_task_ids
            .lock()
            .expect("dispatch start gate should not be poisoned");

        while active_task_ids.contains(task_id) {
            active_task_ids = gate
                .wake_waiters
                .wait(active_task_ids)
                .expect("dispatch start gate should not be poisoned");
        }

        active_task_ids.insert(task_id.to_owned());

        Self {
            task_id: task_id.to_owned(),
        }
    }
}

impl Drop for TaskDispatchStartGuard {
    fn drop(&mut self) {
        let gate = task_dispatch_start_gate();
        let mut active_task_ids = gate
            .active_task_ids
            .lock()
            .expect("dispatch start gate should not be poisoned");
        active_task_ids.remove(&self.task_id);
        gate.wake_waiters.notify_all();
    }
}

fn task_dispatch_start_gate() -> &'static TaskDispatchStartGate {
    static GATE: OnceLock<TaskDispatchStartGate> = OnceLock::new();

    // Dispatch start requests are handled by one long-lived API process in the
    // deployed shape, so a process-local gate is enough to close the race
    // between "no active dispatch exists" and "persist a new preparing record".
    // This keeps the fix lightweight and avoids inventing filesystem locks for
    // a code path that only needs in-process serialization.
    GATE.get_or_init(TaskDispatchStartGate::default)
}

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
