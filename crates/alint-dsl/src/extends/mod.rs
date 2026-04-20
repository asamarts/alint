//! HTTPS-aware `extends:` resolution primitives.
//!
//! Three independent layers:
//!
//! - [`sri`] — parse and verify Subresource Integrity hashes.
//! - [`fetcher`] — blocking HTTPS GET with timeouts + size cap.
//! - [`cache`] — atomic disk cache keyed by SRI.
//!
//! The higher-level `load_recursive` in `lib.rs` composes these
//! to turn an `https://…#sha256-…` entry into a config body.

pub mod cache;
pub mod fetcher;
pub mod sri;

pub use cache::{Cache, CacheError};
pub use fetcher::{FetchError, Fetcher};
pub use sri::{Algorithm, Sri, SriError};

/// Parse an `extends:` URL into the raw URL (stripped of
/// fragment) and the SRI fragment, if any. The fragment is the
/// **only** integrity channel alint recognizes; query strings and
/// headers are ignored.
pub fn split_url_and_sri(entry: &str) -> Result<(String, Option<Sri>), SriError> {
    if let Some((base, fragment)) = entry.split_once('#') {
        let sri = Sri::parse(fragment)?;
        Ok((base.to_string(), Some(sri)))
    } else {
        Ok((entry.to_string(), None))
    }
}

/// Fetch-or-cache: returns the body for `url` given its `sri`,
/// hitting `cache` first. On a cache miss the fetcher runs, the
/// body is SRI-verified, cached, and returned.
pub fn resolve_remote(
    url: &str,
    sri: &Sri,
    fetcher: &Fetcher,
    cache: &Cache,
) -> Result<Vec<u8>, ResolveError> {
    if let Some(bytes) = cache.get(sri)? {
        return Ok(bytes);
    }
    let body = fetcher.get(url)?;
    sri.verify(&body)?;
    cache.put(sri, &body)?;
    Ok(body)
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error(transparent)]
    Sri(#[from] SriError),
    #[error(transparent)]
    Fetch(#[from] FetchError),
    #[error(transparent)]
    Cache(#[from] CacheError),
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY_SHA256: &str =
        "sha256-e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn split_url_with_fragment() {
        let (base, sri) =
            split_url_and_sri(&format!("https://host.example/x.yml#{EMPTY_SHA256}")).unwrap();
        assert_eq!(base, "https://host.example/x.yml");
        assert_eq!(sri.unwrap().encoded(), EMPTY_SHA256);
    }

    #[test]
    fn split_url_without_fragment() {
        let (base, sri) = split_url_and_sri("https://host.example/x.yml").unwrap();
        assert_eq!(base, "https://host.example/x.yml");
        assert!(sri.is_none());
    }

    #[test]
    fn split_url_malformed_fragment_errors() {
        assert!(split_url_and_sri("https://host.example/x.yml#not-a-real-sri").is_err());
    }

    #[test]
    fn resolve_remote_populates_cache_then_serves_from_it() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::thread;

        let tmp = tempfile::tempdir().unwrap();
        let cache = Cache::at(tmp.path().join("cache"));
        let body: Vec<u8> = b"version: 1\nrules: []\n".to_vec();

        // Compute the real SRI of the body so the test is hermetic.
        let sri_str = {
            use sha2::{Digest, Sha256};
            use std::fmt::Write as _;
            let mut h = Sha256::new();
            h.update(&body);
            let digest = h.finalize();
            let mut hex = String::with_capacity(digest.len() * 2);
            for b in &digest {
                write!(hex, "{b:02x}").unwrap();
            }
            format!("sha256-{hex}")
        };
        let sri = Sri::parse(&sri_str).unwrap();

        // Minimal one-shot HTTP server. Only one connection is
        // accepted, so the second `resolve_remote` call MUST hit
        // the cache or the test will hang.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let counter = Arc::new(AtomicUsize::new(0));
        let c2 = counter.clone();
        let server_body = body.clone();
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                c2.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let headers = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                    server_body.len()
                );
                let _ = stream.write_all(headers.as_bytes());
                let _ = stream.write_all(&server_body);
            }
        });

        let url = format!("http://127.0.0.1:{port}/rules.yml");
        let fetcher = Fetcher::default();

        let first = resolve_remote(&url, &sri, &fetcher, &cache).unwrap();
        assert_eq!(first, body);
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        let second = resolve_remote(&url, &sri, &fetcher, &cache).unwrap();
        assert_eq!(second, body);
        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "cache should have served the second call without another GET"
        );
    }
}
