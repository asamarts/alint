//! Blocking HTTPS fetcher for remote `extends:` entries.
//!
//! Uses `ureq` with `rustls` so the binary stays self-contained
//! (no OS-native TLS linking). Request shape is deliberately
//! austere — one-shot GET, timeouts, no redirects beyond the
//! default, no caching (the cache lives a layer up).

use std::io::Read;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Fetcher {
    timeout: Duration,
}

impl Default for Fetcher {
    fn default() -> Self {
        Self::new(Duration::from_secs(30))
    }
}

impl Fetcher {
    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }

    /// GET `url` and return the response body.
    ///
    /// The body is length-capped at 16 MiB to prevent a hostile
    /// server from pinning the linter against memory until the
    /// caller-side timeout fires.
    pub fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(self.timeout))
            // Get non-2xx statuses as a response we can introspect
            // (mapped to our Status variant below) rather than as
            // an opaque Error from .call().
            .http_status_as_error(false)
            .build()
            .into();
        let response = agent.get(url).call().map_err(|e| FetchError::Request {
            url: url.to_string(),
            message: e.to_string(),
        })?;

        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(FetchError::Status {
                url: url.to_string(),
                status,
            });
        }

        let mut body = response.into_body();
        let mut bytes = Vec::new();
        body.as_reader()
            .take(MAX_BODY_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|source| FetchError::Io {
                url: url.to_string(),
                source,
            })?;
        if bytes.len() > MAX_BODY_BYTES {
            return Err(FetchError::TooLarge {
                url: url.to_string(),
                limit: MAX_BODY_BYTES,
            });
        }
        Ok(bytes)
    }
}

const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("GET {url}: {message}")]
    Request { url: String, message: String },
    #[error("GET {url}: HTTP {status}")]
    Status { url: String, status: u16 },
    #[error("GET {url}: body exceeds {limit}-byte ceiling")]
    TooLarge { url: String, limit: usize },
    #[error("GET {url}: body read error: {source}")]
    Io {
        url: String,
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    /// Spin up a minimal one-shot HTTP server on localhost that
    /// serves `body` for `hits` connections, then exits. Returns
    /// the base URL plus a counter tracking how many GETs the
    /// server actually saw.
    fn spawn_http_server(body: Vec<u8>, hits: usize) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let counter = Arc::new(AtomicUsize::new(0));
        let c2 = counter.clone();
        thread::spawn(move || {
            for _ in 0..hits {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                c2.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/plain\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        (format!("http://127.0.0.1:{port}"), counter)
    }

    #[test]
    fn get_returns_200_body() {
        let (url, hits) = spawn_http_server(b"hello from test\n".to_vec(), 1);
        let body = Fetcher::default()
            .get(&format!("{url}/config.yml"))
            .unwrap();
        assert_eq!(body, b"hello from test\n");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn get_errors_on_non_2xx() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
            }
        });
        let err = Fetcher::default()
            .get(&format!("http://127.0.0.1:{port}/missing.yml"))
            .unwrap_err();
        match err {
            FetchError::Status { status: 404, .. } => {}
            other => panic!("expected 404, got {other:?}"),
        }
    }

    #[test]
    fn get_errors_on_connection_refused() {
        // Bind + drop so the port is guaranteed free.
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let err = Fetcher::default()
            .get(&format!("http://127.0.0.1:{port}/x"))
            .unwrap_err();
        assert!(matches!(err, FetchError::Request { .. }), "{err:?}");
    }
}
