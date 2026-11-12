//! Structured inference, asset, and device errors (plan §5.7).
//!
//! Every failure has an explicit category; the CLI maps categories to the
//! existing exit codes without parsing strings. Sensitive values (prompt
//! contents, tokens, credentials, proxy data) must never be embedded in the
//! message payloads.

use std::fmt;

/// Batch-level window failure marker: the scan side answers it with a
/// depth-1 (single-stream) window resubmission (plan §5.3, P4.4 semantics).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferenceError {
    /// The catalog does not register the requested model.
    UnsupportedModel(String),
    /// The model does not offer the requested profile.
    UnsupportedProfile(String),
    /// A built-in manifest failed structural validation.
    InvalidManifest(String),
    /// A required asset file is absent from the cache.
    MissingAsset(String),
    /// A present asset failed its pinned SHA-256 or size check.
    HashMismatch(String),
    /// The model↔adapter↔runtime binding disagrees with the static registry.
    IncompatibleBinding(String),
    /// Downloading is required but the request is offline.
    Offline,
    /// A non-interactive download was attempted without license acceptance.
    LicenseRequired,
    /// The cache volume lacks space for the pinned asset set.
    InsufficientDisk(String),
    /// The HTTPS transport failed.
    Transport(String),
    /// The requested device is unavailable (explicit request, no fallback).
    DeviceUnavailable(String),
    /// Runtime/model initialization failed.
    RuntimeInitialization(String),
    /// A task request failed API validation.
    InvalidTask(String),
    /// The backend failed while executing a valid task.
    BackendExecution(String),
    /// A batch-level window failure (decode/OOM/context overflow): the whole
    /// window is failed and the scan falls back to depth-1 resubmission.
    WindowFailed(String),
    /// The adapter could not decode a backend output.
    AdapterDecode(String),
    /// Another install transaction holds the revision-level lock (review
    /// upstream hardening): the caller must retry after that install ends.
    InstallLocked(String),
    /// Cooperative cancellation observed (never a normal finish reason).
    Cancelled,
}

impl InferenceError {
    /// Stable machine-readable category token (one per variant).
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::UnsupportedModel(_) => "unsupported_model",
            Self::UnsupportedProfile(_) => "unsupported_profile",
            Self::InvalidManifest(_) => "invalid_manifest",
            Self::MissingAsset(_) => "missing_asset",
            Self::HashMismatch(_) => "hash_mismatch",
            Self::IncompatibleBinding(_) => "incompatible_binding",
            Self::Offline => "offline",
            Self::LicenseRequired => "license_required",
            Self::InsufficientDisk(_) => "insufficient_disk",
            Self::Transport(_) => "transport",
            Self::DeviceUnavailable(_) => "device_unavailable",
            Self::RuntimeInitialization(_) => "runtime_initialization",
            Self::InvalidTask(_) => "invalid_task",
            Self::BackendExecution(_) => "backend_execution",
            Self::WindowFailed(_) => "window_failed",
            Self::AdapterDecode(_) => "adapter_decode",
            Self::InstallLocked(_) => "install_locked",
            Self::Cancelled => "cancelled",
        }
    }

    /// The human-readable detail payload, if any.
    #[must_use]
    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::UnsupportedModel(detail)
            | Self::UnsupportedProfile(detail)
            | Self::InvalidManifest(detail)
            | Self::MissingAsset(detail)
            | Self::HashMismatch(detail)
            | Self::IncompatibleBinding(detail)
            | Self::InsufficientDisk(detail)
            | Self::Transport(detail)
            | Self::DeviceUnavailable(detail)
            | Self::RuntimeInitialization(detail)
            | Self::InvalidTask(detail)
            | Self::BackendExecution(detail)
            | Self::WindowFailed(detail)
            | Self::AdapterDecode(detail)
            | Self::InstallLocked(detail) => Some(detail),
            Self::Offline | Self::LicenseRequired | Self::Cancelled => None,
        }
    }
}

impl fmt::Display for InferenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.detail() {
            Some(detail) => write!(f, "{}: {detail}", self.kind()),
            None => f.write_str(self.kind()),
        }
    }
}

impl std::error::Error for InferenceError {}

#[cfg(test)]
mod tests {
    use super::InferenceError;

    #[test]
    fn every_category_has_a_stable_kind_token() {
        let samples = [
            InferenceError::UnsupportedModel("qwen".into()),
            InferenceError::UnsupportedProfile("draft".into()),
            InferenceError::InvalidManifest("bad".into()),
            InferenceError::MissingAsset("model".into()),
            InferenceError::HashMismatch("model".into()),
            InferenceError::IncompatibleBinding("wrapper".into()),
            InferenceError::Offline,
            InferenceError::LicenseRequired,
            InferenceError::InsufficientDisk("1GiB".into()),
            InferenceError::Transport("curl".into()),
            InferenceError::DeviceUnavailable("cuda".into()),
            InferenceError::RuntimeInitialization("load".into()),
            InferenceError::InvalidTask("empty prompt".into()),
            InferenceError::BackendExecution("decode".into()),
            InferenceError::WindowFailed("ctx overflow".into()),
            InferenceError::AdapterDecode("bbox".into()),
            InferenceError::Cancelled,
        ];
        let kinds: Vec<&str> = samples.iter().map(InferenceError::kind).collect();
        let mut unique = kinds.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(kinds.len(), unique.len(), "kind tokens must be unique");
        assert!(samples.iter().all(|error| !error.to_string().is_empty()));
    }

    #[test]
    fn cancelled_is_a_category_without_detail() {
        assert_eq!(InferenceError::Cancelled.to_string(), "cancelled");
        assert_eq!(InferenceError::Cancelled.detail(), None);
    }
}
