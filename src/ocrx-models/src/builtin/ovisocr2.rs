//! The four frozen OvisOCR2 profiles (plan §2.3, T005 checklist).
//!
//! Every value here is migrated verbatim from the legacy pinned constants —
//! never rediscovered from the network. The revision, URLs, SHA-256 values,
//! byte sizes, display metadata, and runtime/adapter bindings are the single
//! source of truth for the built-in catalog.

use ocrx_inference_api::{
    AdapterId, AdapterRequirement, AssetDescriptor, AssetProvenance, AssetRole, AssetScope,
    BindingRequirement, ModelId, ModelManifest, ProfileId, RuntimeId,
};

/// Frozen community revision shared by all four profiles.
pub const OVISOCR2_REVISION: &str = "8a22290a0c42dbcf84739d6d4c2763f877494ae0";

/// Shared revision-scope projector (mmproj): hash, size, and file name.
pub const OVISOCR2_MMPROJ_SHA256: &str =
    "7e371704e8f3c88638830e52866dfa88b2fd383155e3a0f9484e4ce037fdd42f";
pub const OVISOCR2_MMPROJ_BYTES: u64 = 204_987_079;
pub const OVISOCR2_MMPROJ_FILE: &str = "mmproj-F16.gguf";

/// Runtime binding frozen at the Q1.3 pins revision (plan §5.1).
pub const OVISOCR2_RUNTIME_ID: &str = "llama.cpp";
pub const OVISOCR2_WRAPPER_VERSION: &str = "0.1.161";
pub const OVISOCR2_SYS_VERSION: &str = "0.1.161";
pub const OVISOCR2_UPSTREAM_REVISION: &str = "d81235049384534c167caea52b85a694f6103d14";

/// Adapter binding (frozen display pair).
pub const OVISOCR2_ADAPTER_ID: &str = "ovisocr2";
pub const OVISOCR2_ADAPTER_VERSION: &str = "0.1.0";

/// Display metadata shared by all profiles.
pub const OVISOCR2_DISPLAY_NAME: &str = "OvisOCR2";
pub const OVISOCR2_FAMILY: &str = "Ovis";

/// License + provenance (audit metadata).
pub const OVISOCR2_LICENSE_ID: &str = "apache-2.0";
pub const OVISOCR2_PROVIDER: &str = "huggingface";
pub const OVISOCR2_PROVENANCE_URL: &str = "https://huggingface.co/Abiray/OvisOCR2-GGUF";

/// The four profiles in frozen order (sorted for list output elsewhere).
pub const OVISOCR2_PROFILES: [&str; 4] = ["compact", "balanced", "quality", "reference"];

/// Per-profile pinned model asset (file name, sha256, byte size, quantization).
pub struct OvisProfileAsset {
    pub profile: &'static str,
    pub file: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
    pub quantization: &'static str,
}

/// Frozen per-profile model assets (T005: values match the legacy constants).
pub const OVISOCR2_MODEL_ASSETS: [OvisProfileAsset; 4] = [
    OvisProfileAsset {
        profile: "compact",
        file: "OvisOCR2-Q4_K_M.gguf",
        sha256: "4ae54c66bfe1efb2882854ff642c3d137aa082dbd0699bc9234ff39874a492a1",
        bytes: 529_297_280,
        quantization: "Q4_K_M",
    },
    OvisProfileAsset {
        profile: "balanced",
        file: "OvisOCR2-Q5_K_M.gguf",
        sha256: "e17744e367adb891cc9c82f4255408cf46a00efa1b20a83f8187d5bd662ce89a",
        bytes: 577_998_720,
        quantization: "Q5_K_M",
    },
    OvisProfileAsset {
        profile: "quality",
        file: "OvisOCR2-Q8_0.gguf",
        sha256: "3fba6d94312e550575a92d55cffb8d75997bf68a9f133c92ab6ad7f0bf2bc93e",
        bytes: 811_843_498,
        quantization: "Q8_0",
    },
    OvisProfileAsset {
        profile: "reference",
        file: "OvisOCR2-F16.gguf",
        sha256: "bd8f2c62c930d74590abcb5f65f79e898672e458d6044e8b2e885daea948592e",
        bytes: 1_516_744_617,
        quantization: "BF16",
    },
];

/// The pinned community URL base (model files resolve under it).
#[must_use]
pub fn ovisocr2_url_base() -> String {
    format!("https://huggingface.co/Abiray/OvisOCR2-GGUF/resolve/{OVISOCR2_REVISION}")
}

/// Builds the v2 manifest for one concrete Ovis profile.
///
/// # Errors
///
/// Returns [`ocrx_inference_api::InferenceError::UnsupportedProfile`] for a
/// profile outside the frozen four.
pub fn ovisocr2_manifest(profile: &str) -> Result<ModelManifest, ocrx_inference_api::InferenceError> {
    let asset = OVISOCR2_MODEL_ASSETS
        .iter()
        .find(|asset| asset.profile == profile)
        .ok_or_else(|| {
            ocrx_inference_api::InferenceError::UnsupportedProfile(profile.to_owned())
        })?;
    let base = ovisocr2_url_base();
    let assets = vec![
        AssetDescriptor {
            role: AssetRole::new("model").map_err(|error| {
                ocrx_inference_api::InferenceError::InvalidManifest(error.to_string())
            })?,
            required: true,
            scope: AssetScope::Profile,
            relative_path: "model.gguf".to_owned(),
            sha256: asset.sha256.to_owned(),
            byte_len: asset.bytes,
            source_url: format!("{base}/{}", asset.file),
        },
        AssetDescriptor {
            role: AssetRole::new("mmproj").map_err(|error| {
                ocrx_inference_api::InferenceError::InvalidManifest(error.to_string())
            })?,
            required: true,
            scope: AssetScope::Revision,
            relative_path: "mmproj.gguf".to_owned(),
            sha256: OVISOCR2_MMPROJ_SHA256.to_owned(),
            byte_len: OVISOCR2_MMPROJ_BYTES,
            source_url: format!("{base}/{OVISOCR2_MMPROJ_FILE}"),
        },
    ];
    let manifest = ModelManifest {
        schema_version: ModelManifest::SCHEMA_VERSION,
        model: ModelId::new("ovisocr2")
            .map_err(|error| ocrx_inference_api::InferenceError::InvalidManifest(error.to_string()))?,
        profile: ProfileId::new(profile)
            .map_err(|error| ocrx_inference_api::InferenceError::InvalidManifest(error.to_string()))?,
        display_name: OVISOCR2_DISPLAY_NAME.to_owned(),
        family: OVISOCR2_FAMILY.to_owned(),
        quantization: asset.quantization.to_owned(),
        source_revision: OVISOCR2_REVISION.to_owned(),
        assets,
        runtime_binding: BindingRequirement {
            runtime: RuntimeId::new(OVISOCR2_RUNTIME_ID)
                .map_err(|error| ocrx_inference_api::InferenceError::InvalidManifest(error.to_string()))?,
            wrapper_version: OVISOCR2_WRAPPER_VERSION.to_owned(),
            sys_version: OVISOCR2_SYS_VERSION.to_owned(),
            upstream_revision: OVISOCR2_UPSTREAM_REVISION.to_owned(),
        },
        adapter_binding: AdapterRequirement {
            adapter: AdapterId::new(OVISOCR2_ADAPTER_ID)
                .map_err(|error| ocrx_inference_api::InferenceError::InvalidManifest(error.to_string()))?,
            version: OVISOCR2_ADAPTER_VERSION.to_owned(),
        },
        license_id: OVISOCR2_LICENSE_ID.to_owned(),
        provenance: AssetProvenance {
            provider: OVISOCR2_PROVIDER.to_owned(),
            url: OVISOCR2_PROVENANCE_URL.to_owned(),
        },
    };
    manifest
        .validate()
        .map_err(|error| ocrx_inference_api::InferenceError::InvalidManifest(error.to_string()))?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_four_profiles_validate_with_frozen_metadata() {
        for expected in &OVISOCR2_MODEL_ASSETS {
            let manifest = ovisocr2_manifest(expected.profile).unwrap();
            assert_eq!(manifest.validate(), Ok(()));
            assert_eq!(manifest.source_revision, OVISOCR2_REVISION);
            assert_eq!(manifest.display_name, "OvisOCR2");
            assert_eq!(manifest.family, "Ovis");
            assert_eq!(manifest.quantization, expected.quantization);
            assert_eq!(manifest.assets.len(), 2);
            let model = manifest
                .asset(&AssetRole::new("model").unwrap())
                .unwrap();
            assert_eq!(model.sha256, expected.sha256);
            assert_eq!(model.byte_len, expected.bytes);
            assert_eq!(model.scope, AssetScope::Profile);
            assert_eq!(model.relative_path, "model.gguf");
            assert!(model.source_url.ends_with(expected.file));
            let mmproj = manifest
                .asset(&AssetRole::new("mmproj").unwrap())
                .unwrap();
            assert_eq!(mmproj.sha256, OVISOCR2_MMPROJ_SHA256);
            assert_eq!(mmproj.byte_len, OVISOCR2_MMPROJ_BYTES);
            assert_eq!(mmproj.scope, AssetScope::Revision);
            assert_eq!(mmproj.source_url, format!("{}/{}", ovisocr2_url_base(), OVISOCR2_MMPROJ_FILE));
            assert_eq!(manifest.runtime_binding.wrapper_version, "0.1.161");
            assert_eq!(manifest.runtime_binding.sys_version, "0.1.161");
            assert_eq!(
                manifest.runtime_binding.upstream_revision,
                "d81235049384534c167caea52b85a694f6103d14"
            );
            assert_eq!(manifest.adapter_binding.adapter.as_str(), "ovisocr2");
            assert_eq!(manifest.adapter_binding.version, "0.1.0");
        }
    }

    #[test]
    fn unknown_profile_is_rejected_without_network() {
        assert!(matches!(
            ovisocr2_manifest("experimental"),
            Err(ocrx_inference_api::InferenceError::UnsupportedProfile(_))
        ));
    }

    #[test]
    fn mmproj_source_url_is_shared_across_profiles() {
        let compact = ovisocr2_manifest("compact").unwrap();
        let quality = ovisocr2_manifest("quality").unwrap();
        let role = AssetRole::new("mmproj").unwrap();
        assert_eq!(
            compact.asset(&role).unwrap().source_url,
            quality.asset(&role).unwrap().source_url
        );
        assert_ne!(
            compact.asset(&AssetRole::new("model").unwrap()).unwrap().source_url,
            quality.asset(&AssetRole::new("model").unwrap()).unwrap().source_url
        );
    }
}
