//! The frozen OvisOCR3 profiles (plan Phase 0 spike, O3-T01 freeze table).
//!
//! Every value here comes from the spike supply-chain freeze table
//! (`docs/develop/ovisocr3-integration-implementation-log.md`, O3-T01):
//! revision, per-asset SHA-256 values, and byte sizes were verified against
//! the upstream repo via the HF metadata API and re-verified locally after
//! download. The profile set is the **actual available subset** (D-7
//! variable profile set): one community conversion (michalland) publishes
//! exactly one quantization with the F16 projector, so the ladder has a
//! single `quality` rung — the four-rung OvisOCR2 mirror is a desired
//! target, not a hard prerequisite (plan §7 O3-T01 GO rule).

use ocrx_inference_api::{
    AdapterId, AdapterRequirement, AssetDescriptor, AssetProvenance, AssetRole, AssetScope,
    BindingRequirement, InferenceError, ModelId, ModelManifest, ProfileId, RuntimeId,
};

/// Frozen community revision (michalland/OvisOCR3-GGUF, spike O3-T01).
pub const OVISOCR3_REVISION: &str = "7be644a312005c3a55e2c826aee985a5c93d06b2";

/// Revision-scope projector (mmproj, F16): hash, size, and file name.
pub const OVISOCR3_MMPROJ_SHA256: &str =
    "698d199cf44ff10f99825bbd71ad9715e03a6cf7ad041c8ec1db979bfce905cd";
pub const OVISOCR3_MMPROJ_BYTES: u64 = 204_987_136;
pub const OVISOCR3_MMPROJ_FILE: &str = "mmproj-OvisOCR3-F16.gguf";

/// Runtime binding: identical pins to OvisOCR2 — the spike proved the
/// frozen wrapper/sys/upstream loads and generates for OvisOCR3 (O3-T02 GO,
/// D-4 pin-unchanged branch), so `DECODE_GENERATION=3` stays.
pub const OVISOCR3_RUNTIME_ID: &str = "llama.cpp";
pub const OVISOCR3_WRAPPER_VERSION: &str = "0.1.161";
pub const OVISOCR3_SYS_VERSION: &str = "0.1.161";
pub const OVISOCR3_UPSTREAM_REVISION: &str = "d81235049384534c167caea52b85a694f6103d14";

/// Adapter binding (mirrors the future `ocrx-ovisocr3` crate identity).
pub const OVISOCR3_ADAPTER_ID: &str = "ovisocr3";
pub const OVISOCR3_ADAPTER_VERSION: &str = "0.1.0";

/// Display metadata shared by all profiles.
pub const OVISOCR3_DISPLAY_NAME: &str = "OvisOCR3";
pub const OVISOCR3_FAMILY: &str = "Ovis";

/// License + provenance (audit metadata).
pub const OVISOCR3_LICENSE_ID: &str = "apache-2.0";
pub const OVISOCR3_PROVIDER: &str = "huggingface";
pub const OVISOCR3_PROVENANCE_URL: &str = "https://huggingface.co/michalland/OvisOCR3-GGUF";

/// The actual available profile set in declaration order (ascending size;
/// D-7 variable profile set — the spike freeze table's usable subset).
pub const OVISOCR3_PROFILES: [&str; 1] = ["quality"];

/// Per-profile pinned model asset (file name, sha256, byte size, quantization).
pub struct OvisProfileAsset {
    pub profile: &'static str,
    pub file: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
    pub quantization: &'static str,
}

/// Frozen per-profile model assets (spike O3-T01 freeze table, verbatim).
pub const OVISOCR3_MODEL_ASSETS: [OvisProfileAsset; 1] = [OvisProfileAsset {
    profile: "quality",
    file: "OvisOCR3-Q8_0.gguf",
    sha256: "11af29712f08d7f76e6ec06e25d368cc5e25259d51d9fe6cf5ddbb9f1b894ca6",
    bytes: 811_843_712,
    quantization: "Q8_0",
}];

/// The pinned community URL base (model files resolve under it).
#[must_use]
pub fn ovisocr3_url_base() -> String {
    format!("https://huggingface.co/michalland/OvisOCR3-GGUF/resolve/{OVISOCR3_REVISION}")
}

/// Builds the v2 manifest for one concrete OvisOCR3 profile.
///
/// # Errors
///
/// Returns [`InferenceError::UnsupportedProfile`] for a profile outside the
/// frozen available set.
pub fn ovisocr3_manifest(profile: &str) -> Result<ModelManifest, InferenceError> {
    let asset = OVISOCR3_MODEL_ASSETS
        .iter()
        .find(|asset| asset.profile == profile)
        .ok_or_else(|| InferenceError::UnsupportedProfile(profile.to_owned()))?;
    let base = ovisocr3_url_base();
    let assets = vec![
        AssetDescriptor {
            role: AssetRole::new("model")
                .map_err(|error| InferenceError::InvalidManifest(error.to_string()))?,
            required: true,
            scope: AssetScope::Profile,
            relative_path: "model.gguf".to_owned(),
            sha256: asset.sha256.to_owned(),
            byte_len: asset.bytes,
            source_url: format!("{base}/{}", asset.file),
        },
        AssetDescriptor {
            role: AssetRole::new("mmproj")
                .map_err(|error| InferenceError::InvalidManifest(error.to_string()))?,
            required: true,
            scope: AssetScope::Revision,
            relative_path: "mmproj.gguf".to_owned(),
            sha256: OVISOCR3_MMPROJ_SHA256.to_owned(),
            byte_len: OVISOCR3_MMPROJ_BYTES,
            source_url: format!("{base}/{OVISOCR3_MMPROJ_FILE}"),
        },
    ];
    let manifest = ModelManifest {
        schema_version: ModelManifest::SCHEMA_VERSION,
        model: ModelId::new("ovisocr3")
            .map_err(|error| InferenceError::InvalidManifest(error.to_string()))?,
        profile: ProfileId::new(profile)
            .map_err(|error| InferenceError::InvalidManifest(error.to_string()))?,
        display_name: OVISOCR3_DISPLAY_NAME.to_owned(),
        family: OVISOCR3_FAMILY.to_owned(),
        quantization: asset.quantization.to_owned(),
        source_revision: OVISOCR3_REVISION.to_owned(),
        assets,
        runtime_binding: BindingRequirement {
            runtime: RuntimeId::new(OVISOCR3_RUNTIME_ID)
                .map_err(|error| InferenceError::InvalidManifest(error.to_string()))?,
            wrapper_version: OVISOCR3_WRAPPER_VERSION.to_owned(),
            sys_version: OVISOCR3_SYS_VERSION.to_owned(),
            upstream_revision: OVISOCR3_UPSTREAM_REVISION.to_owned(),
        },
        adapter_binding: AdapterRequirement {
            adapter: AdapterId::new(OVISOCR3_ADAPTER_ID)
                .map_err(|error| InferenceError::InvalidManifest(error.to_string()))?,
            version: OVISOCR3_ADAPTER_VERSION.to_owned(),
        },
        license_id: OVISOCR3_LICENSE_ID.to_owned(),
        provenance: AssetProvenance {
            provider: OVISOCR3_PROVIDER.to_owned(),
            url: OVISOCR3_PROVENANCE_URL.to_owned(),
        },
    };
    manifest
        .validate()
        .map_err(|error| InferenceError::InvalidManifest(error.to_string()))?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_available_profiles_validate_with_frozen_metadata() {
        for expected in &OVISOCR3_MODEL_ASSETS {
            let manifest = ovisocr3_manifest(expected.profile).unwrap();
            assert_eq!(manifest.validate(), Ok(()));
            assert_eq!(manifest.model.as_str(), "ovisocr3");
            assert_eq!(manifest.source_revision, OVISOCR3_REVISION);
            assert_eq!(manifest.display_name, "OvisOCR3");
            assert_eq!(manifest.family, "Ovis");
            assert_eq!(manifest.quantization, expected.quantization);
            assert_eq!(manifest.assets.len(), 2);
            let model = manifest.asset(&AssetRole::new("model").unwrap()).unwrap();
            assert_eq!(model.sha256, expected.sha256);
            assert_eq!(model.byte_len, expected.bytes);
            assert_eq!(model.scope, AssetScope::Profile);
            assert_eq!(model.relative_path, "model.gguf");
            assert!(model.source_url.ends_with(expected.file));
            let mmproj = manifest.asset(&AssetRole::new("mmproj").unwrap()).unwrap();
            assert_eq!(mmproj.sha256, OVISOCR3_MMPROJ_SHA256);
            assert_eq!(mmproj.byte_len, OVISOCR3_MMPROJ_BYTES);
            assert_eq!(mmproj.scope, AssetScope::Revision);
            assert_eq!(
                mmproj.source_url,
                format!("{}/{}", ovisocr3_url_base(), OVISOCR3_MMPROJ_FILE)
            );
            // D-4 GO branch: the runtime pin stays frozen at 0.1.161.
            assert_eq!(manifest.runtime_binding.runtime.as_str(), "llama.cpp");
            assert_eq!(manifest.runtime_binding.wrapper_version, "0.1.161");
            assert_eq!(manifest.runtime_binding.sys_version, "0.1.161");
            assert_eq!(
                manifest.runtime_binding.upstream_revision,
                "d81235049384534c167caea52b85a694f6103d14"
            );
            assert_eq!(manifest.adapter_binding.adapter.as_str(), "ovisocr3");
            assert_eq!(manifest.adapter_binding.version, "0.1.0");
            assert_eq!(manifest.license_id, "apache-2.0");
            assert_eq!(
                manifest.provenance.url,
                "https://huggingface.co/michalland/OvisOCR3-GGUF"
            );
        }
    }

    #[test]
    fn profile_set_is_the_actual_available_subset() {
        // D-7: the declared set equals the spike freeze table's usable
        // subset — one rung, declaration order = ascending size.
        assert_eq!(OVISOCR3_PROFILES, ["quality"]);
        assert_eq!(OVISOCR3_MODEL_ASSETS.len(), OVISOCR3_PROFILES.len());
        for (profile, asset) in OVISOCR3_PROFILES.iter().zip(&OVISOCR3_MODEL_ASSETS) {
            assert_eq!(profile, &asset.profile);
        }
    }

    #[test]
    fn unknown_profile_is_rejected_without_network() {
        assert!(matches!(
            ovisocr3_manifest("compact"),
            Err(InferenceError::UnsupportedProfile(_))
        ));
        assert!(matches!(
            ovisocr3_manifest("experimental"),
            Err(InferenceError::UnsupportedProfile(_))
        ));
    }

    #[test]
    fn mmproj_source_url_is_shared_across_profiles() {
        // Single-rung set: the shared-projector semantics still hold — every
        // profile manifest resolves the same revision-scope mmproj URL.
        for profile in OVISOCR3_PROFILES {
            let manifest = ovisocr3_manifest(profile).unwrap();
            let mmproj = manifest.asset(&AssetRole::new("mmproj").unwrap()).unwrap();
            assert_eq!(
                mmproj.source_url,
                format!("{}/{}", ovisocr3_url_base(), OVISOCR3_MMPROJ_FILE)
            );
        }
    }
}
