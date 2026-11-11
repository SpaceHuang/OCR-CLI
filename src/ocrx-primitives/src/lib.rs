//! Business-free technical primitives shared across the pipeline.
//!
//! This crate owns the cancellation token, generation progress, and the
//! SHA-256 implementations that used to live in `ocrx-core`. It must never
//! gain OCR, PDF, or model concepts: consumers (inference API, models,
//! output, scan) depend on it for the plain technical contract only.

pub mod cancellation;
pub mod hash;
pub mod progress;

pub use cancellation::CancellationToken;
pub use hash::{sha256, sha256_file, Sha256State};
pub use progress::GenerationProgress;
