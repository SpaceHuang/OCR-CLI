//! Structured execution identity (plan §5.1).
//!
//! `InferenceIdentity` is produced by the platform after a successful
//! `prepare`. The scan app never fabricates runtime, adapter, or asset
//! hashes: it reads them from here and appends its own prompt/render/PDF
//! route identity.
//!
//! `ModelDisplayMeta` is a **report-only** channel (RBLK-5/RBLK-7): it is
//! stored with the full identity for audit, but pre-comparison and final
//! comparison explicitly exclude it — display text or quantization-label
//! changes never trigger reprocessing. `StaticInferenceIdentity` therefore
//! has no display field at all.

use serde::{Deserialize, Serialize};

use crate::id::{AdapterId, AssetRole, BackendId, ModelId, ProfileId, RuntimeId};

/// One installed/pinned asset inside a model identity (sorted by role).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetIdentity {
    pub role: AssetRole,
    pub sha256: String,
    pub byte_len: u64,
}

/// Resolved model-side identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelIdentity {
    pub model: ModelId,
    pub profile: ProfileId,
    pub source_revision: String,
    /// Sorted by role, no duplicate roles (validated at construction).
    pub assets: Vec<AssetIdentity>,
}

impl ModelIdentity {
    /// Builds a model identity with deterministic asset ordering.
    ///
    /// # Errors
    ///
    /// Returns [`InferenceError::InvalidTask`] for duplicate roles.
    pub fn new(
        model: ModelId,
        profile: ProfileId,
        source_revision: impl Into<String>,
        mut assets: Vec<AssetIdentity>,
    ) -> Result<Self, crate::InferenceError> {
        assets.sort_by(|left, right| left.role.cmp(&right.role));
        for pair in assets.windows(2) {
            if pair[0].role == pair[1].role {
                return Err(crate::InferenceError::InvalidTask(format!(
                    "duplicate asset role {}",
                    pair[0].role
                )));
            }
        }
        Ok(Self {
            model,
            profile,
            source_revision: source_revision.into(),
            assets,
        })
    }
}

/// Runtime-side identity (frozen wrapper/sys/upstream triple).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeIdentity {
    /// Internal runtime id, fixed `llama.cpp` today.
    pub runtime: RuntimeId,
    /// Fixed `0.1.161` (Q1.3 pins revision).
    pub wrapper_version: String,
    /// Fixed `0.1.161` (hard same-version sys dependency).
    pub sys_version: String,
    /// Fixed `d81235049384534c167caea52b85a694f6103d14`
    /// (llama.cpp inside sys 0.1.161, 2026-10-05).
    pub upstream_revision: String,
}

/// Adapter-side identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterIdentity {
    /// Currently `ovisocr2`.
    pub adapter: AdapterId,
    /// Currently `0.1.0`.
    pub version: String,
}

/// Report-only display metadata (RBLK-5/RBLK-7).
///
/// Sole source for `result.json`'s `name`/`family`/`quantization`. Filled
/// once at prepare time from the actually resolved manifest and carried to
/// the scan via the session identity. Explicitly excluded from checkpoint
/// matching: display copy changes never reprocess pages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelDisplayMeta {
    /// Currently `OvisOCR2`.
    pub display_name: String,
    /// Currently `Ovis`.
    pub family: String,
    /// compact=`Q4_K_M`, balanced=`Q5_K_M`, quality=`Q8_0`, reference=`BF16`.
    pub quantization: String,
}

/// Full execution identity after a successful `prepare`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InferenceIdentity {
    pub model: ModelIdentity,
    pub runtime: RuntimeIdentity,
    pub adapter: AdapterIdentity,
    /// Actually executed backend (not the requested one).
    pub backend: BackendId,
    /// Decode generation (D-15): platform-produced, currently frozen at 2.
    pub decode_generation: u32,
    /// Actually effective batch depth: tiered 8/16 (12 GiB threshold),
    /// re-selected to 8 after a GPU→CPU fallback.
    pub batch_depth: usize,
    /// Report-only display metadata (never part of compatibility matching).
    pub display: ModelDisplayMeta,
}

/// The static (prepare-free) subset of the execution identity (RBLK-4).
///
/// Every **execution** field of [`InferenceIdentity`] except the actual
/// backend, the actual batch depth (final-comparison-only fields), and
/// `display` (report-only, RBLK-7). `auto` profiles are resolved through the
/// frozen memory recommendation rule shared with prepare.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaticInferenceIdentity {
    /// Includes the resolved profile, revision, and per-asset manifest
    /// metadata.
    pub model: ModelIdentity,
    /// Static registry runtime binding values.
    pub runtime: RuntimeIdentity,
    /// Static registry adapter binding values.
    pub adapter: AdapterIdentity,
    /// Decode generation (compile-time constant of the platform).
    pub decode_generation: u32,
}

impl StaticInferenceIdentity {
    /// Projection used by the field-completeness test (RBLK-7):
    /// `static identity ∪ {actual backend, actual batch depth} =
    /// execution identity − display`.
    #[must_use]
    pub fn from_execution(identity: &InferenceIdentity) -> Self {
        Self {
            model: identity.model.clone(),
            runtime: identity.runtime.clone(),
            adapter: identity.adapter.clone(),
            decode_generation: identity.decode_generation,
        }
    }
}

/// Runtime telemetry used by the W4.4 GPU/performance report.
///
/// Diagnostic DTO migrated into the public API alongside [`StepProfile`]
/// (plan §3.4/RBLK-3): the session exposes it, the scan reports per-document
/// deltas with [`RuntimeTelemetry::delta_from`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RuntimeTelemetry {
    pub load_time_ms: u64,
    pub first_token_ms: u64,
    pub generated_tokens: u64,
    pub generation_time_ms: u64,
}

impl RuntimeTelemetry {
    #[must_use]
    pub fn output_tokens_per_sec(&self) -> f64 {
        if self.generation_time_ms == 0 {
            0.0
        } else {
            self.generated_tokens as f64 / (self.generation_time_ms as f64 / 1_000.0)
        }
    }

    /// Per-document delta from a baseline snapshot (X1 shared actor): the
    /// runtime accumulates over its whole lifetime, so an orchestrated
    /// document reports `snapshot - baseline` for the additive fields.
    /// `first_token_ms` is a min-since-actor-load and cannot be differenced -
    /// the snapshot passes through (a timing field, excluded from the
    /// structured gpu-resource.json comparison).
    #[must_use]
    pub fn delta_from(&self, base: &Self) -> Self {
        Self {
            load_time_ms: self.load_time_ms.saturating_sub(base.load_time_ms),
            first_token_ms: self.first_token_ms,
            generated_tokens: self.generated_tokens.saturating_sub(base.generated_tokens),
            generation_time_ms: self
                .generation_time_ms
                .saturating_sub(base.generation_time_ms),
        }
    }
}

/// Enablement variable for the step-profile diagnostic channel.
pub const STEP_PROFILE_ENV: &str = "OCRX_STEP_PROFILE";

/// Aggregated per-step profiling snapshot, in microseconds (T1 diagnostic
/// DTO, migrated into the public API per RBLK-3). Accumulated by the
/// inference worker across every windowed decode of the process; the scan
/// reads it through `InferenceSession::step_profile()` and never references
/// the concrete runtime type.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StepProfile {
    pub windows: u64,
    pub steps: u64,
    pub tokens: u64,
    /// Total wall time of the windowed decode functions.
    pub window_total_us: u64,
    pub encode_us: u64,
    pub slot_setup_us: u64,
    pub batch_build_us: u64,
    pub decode_wait_us: u64,
    pub logits_ffi_us: u64,
    pub argmax_us: u64,
    pub detokenize_us: u64,
    pub ring_us: u64,
    /// Window wall time not attributed to any measured segment (progress
    /// messaging, allocations outside segments, slot teardown).
    pub residual_us: u64,
}

impl StepProfile {
    /// Whether profiling was requested through the environment.
    #[must_use]
    pub fn env_enabled() -> bool {
        enabled_from(std::env::var_os(STEP_PROFILE_ENV).as_deref())
    }

    /// Per-document delta from a baseline snapshot (X1 shared actor): every
    /// field is additive, so the orchestrated document reports
    /// `snapshot - baseline` (saturating) instead of the actor's cumulative.
    #[must_use]
    pub fn delta_from(&self, base: &Self) -> Self {
        Self {
            windows: self.windows.saturating_sub(base.windows),
            steps: self.steps.saturating_sub(base.steps),
            tokens: self.tokens.saturating_sub(base.tokens),
            window_total_us: self.window_total_us.saturating_sub(base.window_total_us),
            encode_us: self.encode_us.saturating_sub(base.encode_us),
            slot_setup_us: self.slot_setup_us.saturating_sub(base.slot_setup_us),
            batch_build_us: self.batch_build_us.saturating_sub(base.batch_build_us),
            decode_wait_us: self.decode_wait_us.saturating_sub(base.decode_wait_us),
            logits_ffi_us: self.logits_ffi_us.saturating_sub(base.logits_ffi_us),
            argmax_us: self.argmax_us.saturating_sub(base.argmax_us),
            detokenize_us: self.detokenize_us.saturating_sub(base.detokenize_us),
            ring_us: self.ring_us.saturating_sub(base.ring_us),
            residual_us: self.residual_us.saturating_sub(base.residual_us),
        }
    }

    /// Sums `other` into `self` (the worker accumulates across windows).
    pub fn accumulate(&mut self, other: &Self) {
        self.windows += other.windows;
        self.steps += other.steps;
        self.tokens += other.tokens;
        self.window_total_us += other.window_total_us;
        self.encode_us += other.encode_us;
        self.slot_setup_us += other.slot_setup_us;
        self.batch_build_us += other.batch_build_us;
        self.decode_wait_us += other.decode_wait_us;
        self.logits_ffi_us += other.logits_ffi_us;
        self.argmax_us += other.argmax_us;
        self.detokenize_us += other.detokenize_us;
        self.ring_us += other.ring_us;
        self.residual_us += other.residual_us;
    }

    /// Frozen debug-channel JSON (`step-profile.json`). Field names are a
    /// contract with the phase-2 diagnostic tooling; additions may append,
    /// renames/removals are breaking.
    #[must_use]
    pub fn to_json(&self) -> String {
        format!(
            "{{\"schema_version\":1,\"windows\":{},\"steps\":{},\"tokens\":{},\"us\":{{\"window_total\":{},\"encode\":{},\"slot_setup\":{},\"batch_build\":{},\"decode_wait\":{},\"logits_ffi\":{},\"argmax\":{},\"detokenize\":{},\"ring\":{},\"residual\":{}}}}}",
            self.windows,
            self.steps,
            self.tokens,
            self.window_total_us,
            self.encode_us,
            self.slot_setup_us,
            self.batch_build_us,
            self.decode_wait_us,
            self.logits_ffi_us,
            self.argmax_us,
            self.detokenize_us,
            self.ring_us,
            self.residual_us,
        )
    }
}

fn enabled_from(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|value| !value.is_empty() && value != "0")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(role: &str) -> AssetIdentity {
        AssetIdentity {
            role: AssetRole::new(role).unwrap(),
            sha256: "a".repeat(64),
            byte_len: 10,
        }
    }

    fn full_identity() -> InferenceIdentity {
        InferenceIdentity {
            model: ModelIdentity::new(
                ModelId::new("ovisocr2").unwrap(),
                ProfileId::new("compact").unwrap(),
                "8a22290a",
                vec![asset("mmproj"), asset("model")],
            )
            .unwrap(),
            runtime: RuntimeIdentity {
                runtime: RuntimeId::new("llama.cpp").unwrap(),
                wrapper_version: "0.1.161".to_owned(),
                sys_version: "0.1.161".to_owned(),
                upstream_revision: "d81235049384534c167caea52b85a694f6103d14".to_owned(),
            },
            adapter: AdapterIdentity {
                adapter: AdapterId::new("ovisocr2").unwrap(),
                version: "0.1.0".to_owned(),
            },
            backend: BackendId::new("cpu").unwrap(),
            decode_generation: 2,
            batch_depth: 8,
            display: ModelDisplayMeta {
                display_name: "OvisOCR2".to_owned(),
                family: "Ovis".to_owned(),
                quantization: "Q4_K_M".to_owned(),
            },
        }
    }

    #[test]
    fn model_identity_orders_assets_by_role_and_rejects_duplicates() {
        let identity = full_identity();
        let roles: Vec<&str> = identity
            .model
            .assets
            .iter()
            .map(|asset| asset.role.as_str())
            .collect();
        assert_eq!(roles, ["mmproj", "model"], "assets sorted by role");
        assert!(ModelIdentity::new(
            ModelId::new("ovisocr2").unwrap(),
            ProfileId::new("compact").unwrap(),
            "rev",
            vec![asset("model"), asset("model")],
        )
        .is_err());
    }

    #[test]
    fn identity_serializes_with_deterministic_field_order_and_values() {
        let identity = full_identity();
        let json = serde_json::to_string(&identity).unwrap();
        // Field order is the declaration order (deterministic serialization
        // for byte-stable checkpoint identities); frozen identity values.
        assert!(json.starts_with(
            "{\"model\":{\"model\":\"ovisocr2\",\"profile\":\"compact\",\"source_revision\":\"8a22290a\",\"assets\":[{\"role\":\"mmproj\""
        ));
        assert!(json.contains("\"runtime\":{\"runtime\":\"llama.cpp\",\"wrapper_version\":\"0.1.161\",\"sys_version\":\"0.1.161\",\"upstream_revision\":\"d81235049384534c167caea52b85a694f6103d14\"}"));
        assert!(json.contains("\"backend\":\"cpu\",\"decode_generation\":2,\"batch_depth\":8"));
        // display is serialized (checkpoint audit) ...
        assert!(json.contains("\"display\":{\"display_name\":\"OvisOCR2\""));
        // ... and round-trips.
        let round: InferenceIdentity = serde_json::from_str(&json).unwrap();
        assert_eq!(round, identity);
        // Two serializations of the same identity are byte-identical.
        assert_eq!(json, serde_json::to_string(&identity).unwrap());
    }

    #[test]
    fn static_projection_covers_every_execution_field_except_backend_batch_depth_and_display() {
        // RBLK-7 field completeness: static identity ∪ {actual backend,
        // actual batch depth} = execution identity − display.
        let identity = full_identity();
        let statik = StaticInferenceIdentity::from_execution(&identity);
        assert_eq!(statik.model, identity.model);
        assert_eq!(statik.runtime, identity.runtime);
        assert_eq!(statik.adapter, identity.adapter);
        assert_eq!(statik.decode_generation, identity.decode_generation);
        // The static struct has no display/backend/batch_depth fields by
        // construction; serialization must not carry them either.
        let json = serde_json::to_string(&statik).unwrap();
        assert!(!json.contains("display"));
        assert!(!json.contains("batch_depth"));
        assert!(!json.contains("\"backend\""));
    }

    #[test]
    fn telemetry_delta_from_isolates_per_document_contribution() {
        let base = RuntimeTelemetry {
            load_time_ms: 1_200,
            first_token_ms: 121,
            generated_tokens: 5_000,
            generation_time_ms: 12_000,
        };
        let snapshot = RuntimeTelemetry {
            load_time_ms: 1_200,
            first_token_ms: 100,
            generated_tokens: 7_283,
            generation_time_ms: 16_500,
        };
        let delta = snapshot.delta_from(&base);
        assert_eq!(delta.load_time_ms, 0);
        assert_eq!(delta.generated_tokens, 2_283);
        assert_eq!(delta.generation_time_ms, 4_500);
        assert_eq!(delta.first_token_ms, 100);
        assert_eq!(snapshot.delta_from(&RuntimeTelemetry::default()), snapshot);
        let stale = snapshot.delta_from(&RuntimeTelemetry {
            generated_tokens: 9_999,
            ..RuntimeTelemetry::default()
        });
        assert_eq!(stale.generated_tokens, 0);
    }

    #[test]
    fn telemetry_tokens_per_second_is_zero_without_generation_time() {
        let idle = RuntimeTelemetry::default();
        assert!(idle.output_tokens_per_sec().abs() < 1e-12);
        let busy = RuntimeTelemetry {
            generated_tokens: 2_000,
            generation_time_ms: 1_000,
            ..RuntimeTelemetry::default()
        };
        assert!((busy.output_tokens_per_sec() - 2_000.0).abs() < 1e-9);
    }

    #[test]
    fn step_profile_json_schema_is_frozen() {
        let profile = StepProfile {
            windows: 2,
            steps: 33,
            tokens: 128,
            window_total_us: 1_000_000,
            encode_us: 400_000,
            slot_setup_us: 5_000,
            batch_build_us: 12_000,
            decode_wait_us: 500_000,
            logits_ffi_us: 40_000,
            argmax_us: 30_000,
            detokenize_us: 8_000,
            ring_us: 2_000,
            residual_us: 3_000,
        };
        assert_eq!(
            profile.to_json(),
            "{\"schema_version\":1,\"windows\":2,\"steps\":33,\"tokens\":128,\"us\":{\"window_total\":1000000,\"encode\":400000,\"slot_setup\":5000,\"batch_build\":12000,\"decode_wait\":500000,\"logits_ffi\":40000,\"argmax\":30000,\"detokenize\":8000,\"ring\":2000,\"residual\":3000}}"
        );
        assert_eq!(
            StepProfile::default().to_json(),
            "{\"schema_version\":1,\"windows\":0,\"steps\":0,\"tokens\":0,\"us\":{\"window_total\":0,\"encode\":0,\"slot_setup\":0,\"batch_build\":0,\"decode_wait\":0,\"logits_ffi\":0,\"argmax\":0,\"detokenize\":0,\"ring\":0,\"residual\":0}}"
        );
    }

    #[test]
    fn step_profile_env_enablement_treats_unset_zero_and_empty_as_off() {
        // enabled_from is the single gate predicate for the T1 channel.
        assert!(!enabled_from(None));
        assert!(!enabled_from(Some(std::ffi::OsStr::new("0"))));
        assert!(!enabled_from(Some(std::ffi::OsStr::new(""))));
        assert!(enabled_from(Some(std::ffi::OsStr::new("1"))));
        assert!(enabled_from(Some(std::ffi::OsStr::new("true"))));
        assert!(StepProfile::env_enabled() == enabled_from(
            std::env::var_os(STEP_PROFILE_ENV).as_deref()
        ));
    }

    #[test]
    fn step_profile_delta_from_and_accumulate_cover_every_field() {
        let base = StepProfile {
            windows: 2,
            steps: 10,
            tokens: 100,
            window_total_us: 1_000_000,
            encode_us: 100_000,
            logits_ffi_us: 200_000,
            ..StepProfile::default()
        };
        let snapshot = StepProfile {
            windows: 3,
            steps: 15,
            tokens: 160,
            window_total_us: 1_600_000,
            encode_us: 160_000,
            logits_ffi_us: 260_000,
            argmax_us: 50_000,
            ..StepProfile::default()
        };
        let delta = snapshot.delta_from(&base);
        assert_eq!(delta.windows, 1);
        assert_eq!(delta.steps, 5);
        assert_eq!(delta.tokens, 60);
        assert_eq!(delta.window_total_us, 600_000);
        assert_eq!(delta.encode_us, 60_000);
        assert_eq!(delta.logits_ffi_us, 60_000);
        assert_eq!(delta.argmax_us, 50_000);
        assert_eq!(snapshot.delta_from(&StepProfile::default()), snapshot);

        let mut total = base;
        total.accumulate(&base);
        assert_eq!(total.windows, 4);
        assert_eq!(total.tokens, 200);
        assert_eq!(total.window_total_us, 2_000_000);
    }
}
