//! Scan application (plan §4.1): PDF business orchestration on top of the
//! inference API. Under construction — modules land stage by stage; the
//! CLI keeps its legacy path until T074/T077.

pub mod app;
pub use app::{
    DocumentFailure, DocumentOptions, DocumentOutcome, PdfDocumentAccess, ScanApplication,
    ScanEvent, ScanOutcome,
};
pub mod compatibility;
pub mod ladder;
pub mod native;
pub mod ocr;
pub mod plan;
pub mod render;
pub mod report;
pub mod resume;
pub mod state;
pub mod window;

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod window_tests;
