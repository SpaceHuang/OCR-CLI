//! Public inference platform contract.
//!
//! This crate is the stable, PDF-free boundary between the inference
//! platform and its consumers (the scan application, the CLI, the evaluation
//! module). It owns the common DTOs, identity types, service/session traits,
//! management traits, structured errors, and the internal adapter/backend
//! extension contract.
//!
//! Boundary rules (refactoring plan §3.2): this crate must never depend on
//! `ocrx-core`, `ocrx-pdf`, `ocrx-output`, `ocrx-scan`, `ocrx-eval`, the CLI,
//! or any concrete model/runtime/adapter implementation. No type here may
//! carry page numbers, PDF paths, scan output directories, or OCR parsing.

pub mod asset;
pub mod backend;
pub mod error;
pub mod id;
pub mod identity;
pub mod management;
pub mod service;
pub mod task;

pub use asset::{
    AdapterRequirement, AssetDescriptor, AssetProvenance, AssetScope, BindingRequirement,
    DownloadProposal, InstallInteraction, InstallPolicy, ModelManifest, NonInteractive,
};
pub use backend::{
    BackendHandle, BackendOutput, EncodedTask, ImagePlacement, NativeFinishReason, ProtocolAdapter,
    RuntimeBackend, RuntimeFactory, RuntimeLoadRequest,
};
pub use error::InferenceError;
pub use id::{AdapterId, AssetRole, BackendId, ModelId, ProfileId, RuntimeId};
pub use identity::{
    AdapterIdentity, AssetIdentity, InferenceIdentity, ModelDisplayMeta, ModelIdentity,
    RuntimeIdentity, RuntimeTelemetry, STEP_PROFILE_ENV, StaticInferenceIdentity, StepProfile,
};
pub use management::{
    BindingVerifier, DeviceDiagnostics, DeviceManager, InstallPlan, InstallPlanItem,
    InstallReceipt, ModelDetail, ModelManager, ModelSummary, ProfileRecommendation, ProfileSummary,
    RemoveReport, ResourceSnapshot,
};
pub use service::{InferenceService, InferenceSession, PrepareRequest, PreparedInference};
pub use task::{
    DECODE_GENERATION, DecodeEcho, FinishReason, GenerationParams, ImageInput, RetrySampling,
    SUPPORTED_IMAGE_TOKEN_GRIDS, TaskRequest, TaskResponse, TokenUsage, WINDOW_DEPTH,
    WINDOW_DEPTH_HIGH_VRAM, window_depth_for_vram,
};
