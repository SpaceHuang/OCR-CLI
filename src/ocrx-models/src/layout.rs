//! Layout-model manifest chain (m4-table-reopen plan R2.0 / D-40 / D-45,
//! review RBLK-2 resolution): the PP-DocLayoutV2 detection model rides the
//! same manifest→cache→resolve contract as ovisocr2.
//!
//! Layers:
//! 1. **Disk manifest** (`assets/manifests/pp-doclayout-v2/<revision>/
//!    layout-paddlex.json`, ovisocr2 field style) — embedded verbatim;
//!    preprocessing parameters come from it, never hardcoded at call sites.
//! 2. **Runtime struct** ([`LayoutModelManifest`]) — assets use the shared
//!    [`AssetDescriptor`] contract (role/required/scope/relative_path/
//!    sha256/byte_len/HTTPS-pinned source_url) plus provenance/license.
//! 3. **Registry** — id+revision resolution isomorphic to the ovisocr2
//!    catalog: `pp-doclayout-v2` + the frozen revision → parsed runtime
//!    manifest; anything else is `UnsupportedModel`/`UnsupportedProfile`.
//! 4. **Acquisition chain** ([`ensure_layout_assets`]) — empty cache →
//!    per-asset HTTPS download → sha256 verify → relative-path cache fill;
//!    an existing file with a wrong hash is a hard error (never silently
//!    reused — D-40's degradation to empty regions happens at the scan
//!    side, which catches the error and continues without regions).

use std::path::{Path, PathBuf};

use serde::Deserialize;

use ocrx_inference_api::{
    AssetDescriptor, AssetProvenance, AssetRole, AssetScope, InferenceError, ModelId,
};

use crate::transport::Transport;

/// Registered layout model id (frozen v1: exactly this one).
pub const PADDLE_LAYOUT_MODEL_ID: &str = "pp-doclayout-v2";

/// Frozen source revision of the PP-DocLayoutV2 repo.
pub const PADDLE_LAYOUT_REVISION: &str = "b73668227b14316a38f8b345d6b474e4f1f0b84d";

/// The bundled disk manifest (single source of truth for the chain).
const DISK_MANIFEST_JSON: &str = include_str!(concat!(
    "../../../assets/manifests/pp-doclayout-v2/",
    "b73668227b14316a38f8b345d6b474e4f1f0b84d/layout-paddlex.json"
));

/// Disk manifest fields (ovisocr2 style — see `assets/manifests/`).
#[derive(Debug, Deserialize)]
struct LayoutDiskManifest {
    schema_version: u32,
    model: String,
    source: LayoutDiskSource,
    assets: Vec<LayoutDiskAsset>,
    runtime: LayoutRuntime,
}

#[derive(Debug, Deserialize)]
struct LayoutDiskSource {
    repository: String,
    revision: String,
    license: String,
    source_url: String,
}

#[derive(Debug, Deserialize)]
struct LayoutDiskAsset {
    file: String,
    sha256: String,
    size_bytes: u64,
    role: String,
}

/// Preprocessing/runtime parameters (manifest-provided; never hardcoded at
/// call sites). `route` = the R1-adjudicated runtime route (A = paddle
/// sidecar; route B was eliminated per the preregistered criteria).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct LayoutRuntime {
    pub target_size: u32,
    pub pixel_scale: u32,
    pub conf: f64,
    pub class_table: u32,
    pub route: String,
}

/// Runtime manifest (AssetDescriptor-style assets + provenance/license).
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutModelManifest {
    pub schema_version: u32,
    pub model: ModelId,
    pub source_revision: String,
    pub assets: Vec<AssetDescriptor>,
    pub runtime: LayoutRuntime,
    pub license_id: String,
    pub provenance: AssetProvenance,
}

impl LayoutModelManifest {
    /// The frozen revision-scope cache path of one asset:
    /// `<root>/ocrx/models/<model>/<revision>/<relative_path>`.
    ///
    /// # Errors
    ///
    /// [`InferenceError::InvalidManifest`] for an unknown role or an unsafe
    /// revision/path segment.
    pub fn asset_cache_path(
        &self,
        cache_root: &Path,
        role: &AssetRole,
    ) -> Result<PathBuf, InferenceError> {
        let descriptor = self.asset(role).ok_or_else(|| {
            InferenceError::InvalidManifest(format!("layout manifest has no asset role {role}"))
        })?;
        validate_segment(&self.source_revision)?;
        validate_segment(&descriptor.relative_path)?;
        Ok(cache_root
            .join("ocrx")
            .join("models")
            .join(self.model.as_str())
            .join(&self.source_revision)
            .join(&descriptor.relative_path))
    }

    fn asset(&self, role: &AssetRole) -> Option<&AssetDescriptor> {
        self.assets.iter().find(|asset| &asset.role == role)
    }

    /// Structural validation (the shared manifest invariants plus the
    /// layout-specific sanity bounds).
    ///
    /// # Errors
    ///
    /// [`InferenceError::InvalidManifest`] with the first violated rule.
    pub fn validate(&self) -> Result<(), InferenceError> {
        if self.schema_version != 1 {
            return Err(InferenceError::InvalidManifest(
                "layout manifest schema_version must be 1".into(),
            ));
        }
        if self.license_id.trim().is_empty() {
            return Err(InferenceError::InvalidManifest(
                "empty license field".into(),
            ));
        }
        if self.assets.is_empty() {
            return Err(InferenceError::InvalidManifest(
                "layout manifest has no assets".into(),
            ));
        }
        for asset in &self.assets {
            if asset.sha256.len() != 64 || !asset.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(InferenceError::InvalidManifest(format!(
                    "asset {} has a non-sha256 hash",
                    asset.role
                )));
            }
            if !asset.source_url.starts_with("https://") {
                return Err(InferenceError::InvalidManifest(format!(
                    "asset {} source_url is not HTTPS-pinned",
                    asset.role
                )));
            }
            if validate_segment(&asset.relative_path).is_err() {
                return Err(InferenceError::InvalidManifest(format!(
                    "asset {} has an unsafe relative path {:?}",
                    asset.role, asset.relative_path
                )));
            }
            if !asset.required {
                return Err(InferenceError::InvalidManifest(format!(
                    "asset {} must be required (detection degrades without it)",
                    asset.role
                )));
            }
        }
        if self.runtime.target_size == 0 || self.runtime.pixel_scale == 0 {
            return Err(InferenceError::InvalidManifest(
                "runtime target_size/pixel_scale must be positive".into(),
            ));
        }
        if !(0.0..=1.0).contains(&self.runtime.conf) {
            return Err(InferenceError::InvalidManifest(
                "runtime conf must be within [0, 1]".into(),
            ));
        }
        if self.runtime.class_table == 0 {
            return Err(InferenceError::InvalidManifest(
                "runtime class_table must be positive".into(),
            ));
        }
        if self.runtime.route != "A" {
            return Err(InferenceError::InvalidManifest(format!(
                "layout route {:?} is not the R1-adjudicated route A",
                self.runtime.route
            )));
        }
        Ok(())
    }
}

/// A safe single cache path segment (mirrors the shared manifest rule).
fn validate_segment(segment: &str) -> Result<(), InferenceError> {
    let unsafe_segment = segment.is_empty()
        || segment == "."
        || segment == ".."
        || Path::new(segment).is_absolute()
        || segment.contains('/')
        || segment.contains('\\')
        || segment.contains(':')
        || segment.chars().any(char::is_whitespace);
    if unsafe_segment {
        Err(InferenceError::InvalidManifest(format!(
            "unsafe path segment {segment:?}"
        )))
    } else {
        Ok(())
    }
}

/// Registry resolution: `pp-doclayout-v2` + frozen revision → the parsed,
/// validated runtime manifest (isomorphic to the ovisocr2 id+profile
/// resolution; the disk manifest is the parse source, per D-45).
///
/// # Errors
///
/// [`InferenceError::UnsupportedModel`] for an unregistered id,
/// [`InferenceError::UnsupportedProfile`] for a foreign revision, and
/// [`InferenceError::InvalidManifest`] when the bundled manifest fails
/// validation (a build-time regression, surfaced here to keep the single
/// source of truth).
pub fn paddlex_layout_manifest(
    id: &str,
    revision: Option<&str>,
) -> Result<LayoutModelManifest, InferenceError> {
    if id != PADDLE_LAYOUT_MODEL_ID {
        return Err(InferenceError::UnsupportedModel(id.to_owned()));
    }
    if let Some(revision) = revision {
        if revision != PADDLE_LAYOUT_REVISION {
            return Err(InferenceError::UnsupportedProfile(format!(
                "layout revision {revision} is not the frozen {PADDLE_LAYOUT_REVISION}"
            )));
        }
    }
    let disk: LayoutDiskManifest = serde_json::from_str(DISK_MANIFEST_JSON).map_err(|error| {
        InferenceError::InvalidManifest(format!("bundled layout manifest is malformed: {error}"))
    })?;
    if disk.schema_version != 1 || disk.model != PADDLE_LAYOUT_MODEL_ID {
        return Err(InferenceError::InvalidManifest(
            "bundled layout manifest identity mismatch".into(),
        ));
    }
    if disk.source.revision != PADDLE_LAYOUT_REVISION {
        return Err(InferenceError::InvalidManifest(
            "bundled layout manifest revision drift".into(),
        ));
    }
    let model = ModelId::new(&disk.model)
        .map_err(|_| InferenceError::InvalidManifest("layout model id is invalid".into()))?;
    let base = format!(
        "{}/resolve/{}",
        disk.source.source_url.trim_end_matches('/'),
        disk.source.revision
    );
    let assets = disk
        .assets
        .iter()
        .map(|asset| {
            let role = AssetRole::new(&asset.role)
                .map_err(|_| InferenceError::InvalidManifest(format!("bad role {asset:?}")))?;
            Ok(AssetDescriptor {
                role,
                required: true,
                scope: AssetScope::Revision,
                relative_path: asset.file.clone(),
                sha256: asset.sha256.to_ascii_lowercase(),
                byte_len: asset.size_bytes,
                source_url: format!("{base}/{}", asset.file),
            })
        })
        .collect::<Result<Vec<_>, InferenceError>>()?;
    let manifest = LayoutModelManifest {
        schema_version: disk.schema_version,
        model,
        source_revision: disk.source.revision,
        assets,
        runtime: disk.runtime,
        license_id: disk.source.license,
        provenance: AssetProvenance {
            provider: "hf-mirror".to_owned(),
            url: disk.source.source_url,
        },
    };
    manifest.validate()?;
    Ok(manifest)
}

/// Acquisition chain (D-45 acceptance path): every asset present+verified
/// in the cache is reused; a missing one is downloaded from its pinned
/// HTTPS URL, hash-verified, and atomically moved into place; an existing
/// file with a wrong hash is a hard [`InferenceError::HashMismatch`] (never
/// silently overwritten — a tampered cache must be cleaned, not trusted).
/// Returns the resolved cache paths in manifest order.
///
/// # Errors
///
/// [`InferenceError::Transport`]/[`InferenceError::HashMismatch`]/
/// [`InferenceError::InvalidManifest`] as described.
pub fn ensure_layout_assets(
    cache_root: &Path,
    manifest: &LayoutModelManifest,
    transport: &dyn Transport,
) -> Result<Vec<PathBuf>, InferenceError> {
    manifest.validate()?;
    let mut resolved = Vec::new();
    for asset in &manifest.assets {
        let path = manifest.asset_cache_path(cache_root, &asset.role)?;
        if path.exists() {
            verify_file(&path, &asset.sha256, asset.byte_len)?;
        } else {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    InferenceError::Transport(format!("cache mkdir failed: {error}"))
                })?;
            }
            let part = path.with_extension("part");
            transport.download(&asset.source_url, &part)?;
            verify_file(&part, &asset.sha256, asset.byte_len)?;
            std::fs::rename(&part, &path).map_err(|error| {
                InferenceError::Transport(format!("cache commit failed: {error}"))
            })?;
        }
        resolved.push(path);
    }
    Ok(resolved)
}

fn verify_file(
    path: &Path,
    expected_sha256: &str,
    expected_len: u64,
) -> Result<(), InferenceError> {
    let len = std::fs::metadata(path)
        .map_err(|error| InferenceError::MissingAsset(format!("{}: {error}", path.display())))?
        .len();
    if len != expected_len {
        return Err(InferenceError::HashMismatch(format!(
            "{}: expected {expected_len} bytes, found {len}",
            path.display()
        )));
    }
    let digest = ocrx_primitives::sha256_file(path).map_err(|error| {
        InferenceError::HashMismatch(format!("{}: read failed: {error}", path.display()))
    })?;
    if digest != expected_sha256 {
        return Err(InferenceError::HashMismatch(format!(
            "{}: expected {expected_sha256}, got {digest}",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::FakeTransport;

    fn manifest() -> LayoutModelManifest {
        paddlex_layout_manifest(PADDLE_LAYOUT_MODEL_ID, None).expect("bundled manifest resolves")
    }

    fn cache(test: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ocrx-layout-{}-{test}", std::process::id()))
    }

    fn write_asset(cache_root: &Path, manifest: &LayoutModelManifest, role: &str, data: &[u8]) {
        let path = manifest
            .asset_cache_path(cache_root, &AssetRole::new(role).unwrap())
            .unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, data).unwrap();
    }

    #[test]
    fn bundled_manifest_resolves_with_the_frozen_identity() {
        let manifest = manifest();
        assert_eq!(manifest.model.as_str(), "pp-doclayout-v2");
        assert_eq!(manifest.source_revision, PADDLE_LAYOUT_REVISION);
        assert_eq!(manifest.schema_version, 1);
        assert_eq!(manifest.license_id, "apache-2.0");
        assert_eq!(manifest.provenance.provider, "hf-mirror");
        // Both assets ride the shared descriptor contract.
        assert_eq!(manifest.assets.len(), 2);
        let weights = manifest
            .asset(&AssetRole::new("layout-weights").unwrap())
            .unwrap();
        assert_eq!(
            weights.sha256,
            "45404a84c9fdf91d7bbc94bd47ac4c03649bda84167de04c62bff4726657869a"
        );
        assert_eq!(weights.byte_len, 212_170_944);
        assert_eq!(weights.relative_path, "inference.pdiparams");
        assert!(weights.required);
        assert_eq!(weights.scope, AssetScope::Revision);
        assert!(
            weights
                .source_url
                .starts_with("https://hf-mirror.com/PaddlePaddle/PP-DocLayoutV2/resolve/")
        );
        // Preprocessing params come from the manifest (never call-site
        // hardcoded) — the values the B0'/pairing runs actually used.
        assert_eq!(manifest.runtime.target_size, 800);
        assert_eq!(manifest.runtime.pixel_scale, 255);
        assert!((manifest.runtime.conf - 0.25).abs() < 1e-9);
        assert_eq!(manifest.runtime.class_table, 21);
        assert_eq!(manifest.runtime.route, "A");
        assert_eq!(manifest.validate(), Ok(()));
    }

    #[test]
    fn registry_rejects_foreign_ids_and_revisions() {
        assert!(matches!(
            paddlex_layout_manifest("mineru-layout", None),
            Err(InferenceError::UnsupportedModel(_))
        ));
        assert!(matches!(
            paddlex_layout_manifest(PADDLE_LAYOUT_MODEL_ID, Some("deadbeef")),
            Err(InferenceError::UnsupportedProfile(_))
        ));
    }

    /// A synthetic two-asset manifest for cache-chain tests: the hashes are
    /// computed from the test bytes at runtime, so downloads can serve exact
    /// content. (The bundled manifest's frozen hashes are identity-checked
    /// above; the chain mechanics are manifest-driven and shared.)
    fn synthetic_manifest() -> LayoutModelManifest {
        let graph = b"layout graph bytes".to_vec();
        let asset = |role: &str, file: &str, bytes: &[u8]| AssetDescriptor {
            role: AssetRole::new(role).unwrap(),
            required: true,
            scope: AssetScope::Revision,
            relative_path: file.to_owned(),
            sha256: ocrx_primitives::sha256(bytes),
            byte_len: bytes.len() as u64,
            source_url: format!("https://hf-mirror.invalid/ppv2/resolve/testrev/{file}"),
        };
        LayoutModelManifest {
            schema_version: 1,
            model: ModelId::new(PADDLE_LAYOUT_MODEL_ID).unwrap(),
            source_revision: "testrev".to_owned(),
            assets: vec![asset("layout-graph", "inference.json", &graph)],
            runtime: LayoutRuntime {
                target_size: 800,
                pixel_scale: 255,
                conf: 0.25,
                class_table: 21,
                route: "A".to_owned(),
            },
            license_id: "apache-2.0".to_owned(),
            provenance: AssetProvenance {
                provider: "hf-mirror".to_owned(),
                url: "https://hf-mirror.invalid/ppv2".to_owned(),
            },
        }
    }

    #[test]
    fn cache_paths_are_revision_scoped_under_the_layout_namespace() {
        let manifest = manifest();
        let root = Path::new("/cache");
        let path = manifest
            .asset_cache_path(root, &AssetRole::new("layout-graph").unwrap())
            .unwrap();
        assert_eq!(
            path,
            Path::new("/cache")
                .join("ocrx/models/pp-doclayout-v2")
                .join(PADDLE_LAYOUT_REVISION)
                .join("inference.json")
        );
        assert!(
            manifest
                .asset_cache_path(root, &AssetRole::new("model").unwrap())
                .is_err()
        );
    }

    #[test]
    fn acquisition_chain_downloads_verifies_and_commits() {
        // D-45 acceptance path (offline form): empty cache → pinned-URL
        // download → sha256 verify → relative-path fill → resolved paths.
        let manifest = synthetic_manifest();
        let root = cache("acquisition");
        let _ = std::fs::remove_dir_all(&root);
        let bytes = b"layout graph bytes";
        let fake = FakeTransport::new();
        fake.serve(&manifest.assets[0].source_url, bytes);
        let paths = ensure_layout_assets(&root, &manifest, &fake).unwrap();
        assert_eq!(paths.len(), 1);
        assert_eq!(std::fs::read(&paths[0]).unwrap(), bytes);
        assert_eq!(
            fake.requested_urls(),
            vec![manifest.assets[0].source_url.clone()]
        );
        // No .part litter next to the committed asset.
        assert!(!paths[0].with_extension("part").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn wrong_served_bytes_fail_the_hash_gate_before_commit() {
        let manifest = synthetic_manifest();
        let root = cache("wrong_bytes");
        let _ = std::fs::remove_dir_all(&root);
        let fake = FakeTransport::new();
        fake.serve(&manifest.assets[0].source_url, b"corrupted");
        assert!(matches!(
            ensure_layout_assets(&root, &manifest, &fake),
            Err(InferenceError::HashMismatch(_))
        ));
        // Nothing committed — neither the asset nor a .part remnant.
        assert!(
            !manifest
                .asset_cache_path(&root, &manifest.assets[0].role)
                .unwrap()
                .exists()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn existing_verified_assets_are_reused_without_network() {
        let manifest = synthetic_manifest();
        let root = cache("reuse");
        let _ = std::fs::remove_dir_all(&root);
        write_asset(&root, &manifest, "layout-graph", b"layout graph bytes");
        let fake = FakeTransport::new();
        let paths = ensure_layout_assets(&root, &manifest, &fake).unwrap();
        assert_eq!(paths.len(), 1);
        assert!(fake.requested_urls().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn tampered_cache_is_a_hard_error_never_silent_reuse() {
        let manifest = synthetic_manifest();
        let root = cache("tamper");
        let _ = std::fs::remove_dir_all(&root);
        write_asset(&root, &manifest, "layout-graph", b"tampered");
        let fake = FakeTransport::new();
        assert!(matches!(
            ensure_layout_assets(&root, &manifest, &fake),
            Err(InferenceError::HashMismatch(_))
        ));
        assert!(fake.requested_urls().is_empty(), "no re-download");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn validation_rejects_non_route_a_and_bad_bounds() {
        let mut manifest = manifest();
        manifest.runtime.route = "B".to_owned();
        assert!(manifest.validate().is_err());
        manifest.runtime.route = "A".to_owned();
        manifest.runtime.conf = 1.5;
        assert!(manifest.validate().is_err());
        manifest.runtime.conf = 0.25;
        manifest.assets[0].sha256 = "zz".to_owned();
        assert!(manifest.validate().is_err());
    }
}
