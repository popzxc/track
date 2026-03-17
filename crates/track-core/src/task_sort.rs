use crate::types::Task;

pub fn sort_tasks(tasks: &[Task]) -> Vec<Task> {
    let mut sorted = tasks.to_vec();
    sorted.sort_by(|left, right| {
        left.status
            .cmp(&right.status)
            .then_with(|| left.priority.cmp(&right.priority))
            .then_with(|| right.created_at.cmp(&left.created_at))
    });
    sorted
}
