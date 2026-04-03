use std::time::Duration;

use track_config::runtime::ApiRuntimeConfig;

const NOTIFY_TIMEOUT: Duration = Duration::from_millis(250);

// =============================================================================
// Local API Notification
// =============================================================================
//
// The CLI owns task creation, but the browser UI should still refresh when a
// new task lands on disk. We keep that integration deliberately lightweight:
// the CLI performs a best-effort POST to the local API, and the caller decides
// whether failures matter. For this project they do not block task capture.
//
// The route stays tiny and local-only, but a dedicated HTTP client is still
// the better tool for the job. That keeps the integration reliable without
// forcing this crate to maintain its own miniature HTTP implementation.
pub fn notify_task_changed(api: &ApiRuntimeConfig) -> Result<(), ureq::Error> {
    let url = format!("http://127.0.0.1:{}/api/events/tasks-changed", api.port);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(NOTIFY_TIMEOUT))
        .build()
        .into();

    agent.post(&url).send_empty()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use track_config::runtime::ApiRuntimeConfig;

    use super::notify_task_changed;

    #[test]
    fn posts_task_change_notification_to_the_local_api() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let port = listener
            .local_addr()
            .expect("listener should expose local address")
            .port();
        let (sender, receiver) = mpsc::channel();

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("listener should accept");
            let mut buffer = [0u8; 1024];
            let bytes_read = stream
                .read(&mut buffer)
                .expect("request should be readable");
            stream
                .write_all(
                    b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .expect("response should be writable");
            sender
                .send(String::from_utf8_lossy(&buffer[..bytes_read]).into_owned())
                .expect("request should reach the test thread");
        });

        notify_task_changed(&ApiRuntimeConfig { port }).expect("notification should succeed");

        let request = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("notification should be received");

        // The observable contract we care about is the route and verb. Header
        // framing belongs to the HTTP client implementation and can vary
        // across versions without changing the integration behavior we rely on.
        assert!(request.starts_with("POST /api/events/tasks-changed HTTP/1.1"));
        server.join().expect("server thread should exit cleanly");
    }
}
