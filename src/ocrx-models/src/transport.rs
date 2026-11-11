//! Download transport abstraction (plan §8.5): production uses the pinned
//! HTTPS-only curl process; tests inject a local fake — nothing touches the
//! network.

use std::path::Path;

use ocrx_inference_api::InferenceError;

/// Byte-source abstraction for asset downloads.
pub trait Transport {
    /// Downloads `url` into `destination` (a fresh `.part` path chosen by
    /// the caller). The caller verifies the hash before committing.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::Transport`] for failures;
    /// implementations must refuse non-HTTPS URLs.
    fn download(&self, url: &str, destination: &Path) -> Result<(), InferenceError>;
}

/// HTTPS-only curl production transport (same frozen flags as the legacy
/// `download_asset`: unconditional GET, no ETag, proto downgrade blocked).
#[derive(Debug, Default, Clone, Copy)]
pub struct CurlTransport;

impl Transport for CurlTransport {
    fn download(&self, url: &str, destination: &Path) -> Result<(), InferenceError> {
        // Provenance contract (AGENTS.md): weights come only from pinned
        // HTTPS URLs; the proto flags also stop an https→http redirect
        // downgrade mid-transfer.
        if !url.starts_with("https://") {
            return Err(InferenceError::Transport(format!(
                "non-HTTPS asset source refused: {url}"
            )));
        }
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                InferenceError::Transport(format!("unable to create destination: {error}"))
            })?;
        }
        let output = std::process::Command::new("curl")
            .args([
                "--fail",
                "--location",
                "--proto-default",
                "https",
                "--proto-redir",
                "https",
                "--silent",
                "--show-error",
                "--retry",
                "2",
                "--output",
                destination.to_str().ok_or_else(|| {
                    InferenceError::Transport("destination path is not UTF-8".into())
                })?,
                url,
            ])
            .output()
            .map_err(|error| InferenceError::Transport(error.to_string()))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(InferenceError::Transport(format!(
                "curl failed with status {:?}",
                output.status.code()
            )))
        }
    }
}

/// One fake route: URL prefix → controlled bytes or controlled failure.
pub type FakeRoute = (String, Result<Vec<u8>, InferenceError>);

/// In-memory fake transport for offline tests: replays controlled bytes or
/// controlled failures per URL prefix (longest matching prefix wins).
#[derive(Debug, Default)]
pub struct FakeTransport {
    pub responses: std::sync::Mutex<Vec<FakeRoute>>,
    /// URLs that were requested (asserted by tests: no implicit network).
    pub requested: std::sync::Mutex<Vec<String>>,
}

impl FakeTransport {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replays `bytes` for every URL starting with `prefix`.
    ///
    /// # Panics
    ///
    /// Only if the internal mutex is poisoned (no code path panics while
    /// holding it).
    pub fn serve(&self, prefix: &str, bytes: &[u8]) {
        self.responses
            .lock()
            .expect("fake transport lock")
            .push((prefix.to_owned(), Ok(bytes.to_vec())));
    }

    /// Fails every URL starting with `prefix`.
    ///
    /// # Panics
    ///
    /// Only if the internal mutex is poisoned (no code path panics while
    /// holding it).
    pub fn fail(&self, prefix: &str, error: InferenceError) {
        self.responses
            .lock()
            .expect("fake transport lock")
            .push((prefix.to_owned(), Err(error)));
    }

    /// URLs that went through [`Transport::download`], in request order.
    ///
    /// # Panics
    ///
    /// Only if the internal mutex is poisoned (no code path panics while
    /// holding it).
    #[must_use]
    pub fn requested_urls(&self) -> Vec<String> {
        self.requested.lock().expect("fake transport lock").clone()
    }
}

impl Transport for FakeTransport {
    fn download(&self, url: &str, destination: &Path) -> Result<(), InferenceError> {
        self.requested
            .lock()
            .expect("fake transport lock")
            .push(url.to_owned());
        let responses = self.responses.lock().expect("fake transport lock");
        // Longest matching prefix wins so `/model` never shadows `/model-extra`.
        let (_, response) = responses
            .iter()
            .filter(|(prefix, _)| url.starts_with(prefix))
            .max_by_key(|(prefix, _)| prefix.len())
            .ok_or_else(|| {
                InferenceError::Transport(format!("fake transport has no route for {url}"))
            })?;
        match response {
            Ok(bytes) => {
                if let Some(parent) = destination.parent() {
                    std::fs::create_dir_all(parent).map_err(|error| {
                        InferenceError::Transport(error.to_string())
                    })?;
                }
                std::fs::write(destination, bytes)
                    .map_err(|error| InferenceError::Transport(error.to_string()))
            }
            Err(error) => Err(error.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curl_transport_refuses_non_https_urls_before_spawning_a_process() {
        let transport = CurlTransport;
        let destination = std::env::temp_dir().join("ocrx-curl-transport-should-not-run.part");
        let error = transport
            .download("http://example.invalid/model.gguf", &destination)
            .unwrap_err();
        assert!(matches!(error, InferenceError::Transport(detail) if detail.contains("non-HTTPS")));
        assert!(transport
            .download("file:///tmp/model", &destination)
            .is_err());
        assert!(!destination.exists(), "no process may have run");
    }

    #[test]
    fn fake_transport_replays_bytes_and_records_requests() {
        let transport = FakeTransport::new();
        transport.serve("https://example.invalid/", b"asset-bytes");
        let destination = std::env::temp_dir().join(format!("ocrx-fake-transport-{}.part", std::process::id()));
        transport
            .download("https://example.invalid/model.gguf", &destination)
            .unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"asset-bytes");
        assert_eq!(
            transport.requested_urls(),
            vec!["https://example.invalid/model.gguf".to_owned()]
        );
        let _ = std::fs::remove_file(destination);
    }

    #[test]
    fn fake_transport_surfaces_controlled_failures_and_unknown_routes() {
        let transport = FakeTransport::new();
        transport.fail(
            "https://fail.invalid/",
            InferenceError::Transport("boom".into()),
        );
        let destination = std::env::temp_dir().join("ocrx-fake-transport-fail.part");
        match transport.download("https://fail.invalid/model", &destination) {
            Err(InferenceError::Transport(detail)) => assert_eq!(detail, "boom"),
            other => panic!("expected transport failure, got {other:?}"),
        }
        assert!(transport
            .download("https://unrouted.invalid/model", &destination)
            .is_err());
    }
}
