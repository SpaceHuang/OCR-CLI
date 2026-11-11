//! Scan application (plan §4.1): PDF business orchestration on top of the
//! inference API. Under construction — modules land stage by stage; the
//! CLI keeps its legacy path until T074/T077.

pub mod ladder;
pub mod ocr;
pub mod state;
pub mod window;
pub mod compatibility;
pub mod resume;
pub mod render;
pub mod multi_doc;
pub mod report;
pub mod telemetry;
pub mod plan;
pub mod native;
