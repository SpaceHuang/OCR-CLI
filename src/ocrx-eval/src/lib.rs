//! Quality evaluation module (plan §4.1 ocrx-eval): truth manifests,
//! frozen metrics/gates, the two execution entries (full scan / model
//!专项), and the byte-stable rescan comparison primitive (§3.5 item 10).

pub mod execute;
pub mod quality;

pub use execute::{
    ArtifactPair, EvaluationIdentity, RescanComparison, ScanEvaluationEntry,
    compare_rescan_artifacts, compare_rescan_outputs,
};
pub use quality::*;
