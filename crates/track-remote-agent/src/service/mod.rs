mod dispatch;
mod lifecycle;
mod remote_agent_services;
mod review;

pub use self::dispatch::RemoteDispatchService;
pub use self::remote_agent_services::RemoteAgentRuntimeServices;
pub use self::review::RemoteReviewService;

const REMOTE_FAILURE_LOG_TAIL_LINES: usize = 30;

fn format_log_output_tail(output: &str) -> Option<String> {
    let lines = output
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return None;
    }

    let tail = lines
        .into_iter()
        .rev()
        .take(REMOTE_FAILURE_LOG_TAIL_LINES)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>();

    Some(tail.join("\n"))
}

fn log_remote_failure_output(stdout: Option<&str>, stderr: Option<&str>) {
    if let Some(stderr_tail) = stderr.and_then(format_log_output_tail) {
        tracing::error!(stderr_tail = %stderr_tail, "Remote command stderr tail");
    }

    if let Some(stdout_tail) = stdout.and_then(format_log_output_tail) {
        tracing::error!(stdout_tail = %stdout_tail, "Remote command stdout tail");
    }
}

#[cfg(test)]
mod tests;
