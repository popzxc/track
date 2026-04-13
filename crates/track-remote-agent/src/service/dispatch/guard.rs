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
