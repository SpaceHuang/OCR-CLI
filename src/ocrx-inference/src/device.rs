//! Device management (plan §5.6/§8.8): hardware/compiled-capability
//! intersection, frozen `auto` priority, batch-depth tiering, and doctor
//! diagnostics.

use ocrx_inference_api::{
    BackendId, DeviceDiagnostics, DeviceManager, InferenceError, ResourceSnapshot,
};
use ocrx_runtime::Backend;

use crate::probe::SystemProbe;

/// Production device manager over an injected system probe (T039: the
/// probe and the compiled-capability intersection moved here from the CLI).
pub struct DeviceManagerImpl {
    probe: Box<dyn SystemProbe>,
    /// Environment override for the memory report (`OCRX_MEMORY_GB`).
    env_memory_gb: Option<u64>,
    /// Cache root for the doctor install overview.
    cache_root: std::path::PathBuf,
}

impl DeviceManagerImpl {
    #[must_use]
    pub fn new(probe: Box<dyn SystemProbe>, env_memory_gb: Option<u64>, cache_root: std::path::PathBuf) -> Self {
        Self {
            probe,
            env_memory_gb,
            cache_root,
        }
    }

    /// Builds the availability snapshot: hardware presence intersected with
    /// the compiled-in backend set (W1.1) — `auto` and `doctor` can never
    /// select a backend that is not compiled in.
    #[must_use]
    pub fn snapshot_with_probe(&self, probe: &dyn SystemProbe) -> ResourceSnapshot {
        // Frozen resolution: env override > probe > default 16 (legacy
        // `resolve_config` semantics, W1.2).
        let memory_gb = self
            .env_memory_gb
            .or_else(|| probe.detected_memory_gb())
            .unwrap_or(16);
        ResourceSnapshot {
            memory_gb,
            metal_available: probe.metal_available() && Backend::Metal.is_compiled(),
            cuda_available: probe.cuda_available() && Backend::Cuda.is_compiled(),
            vulkan_available: probe.vulkan_available() && Backend::Vulkan.is_compiled(),
            cpu_available: probe.cpu_available() && Backend::Cpu.is_compiled(),
        }
    }

    /// VRAM probe for the tiering decision (`None` = CPU or unavailable).
    #[must_use]
    pub fn vram_bytes(&self, device: &BackendId) -> Option<u64> {
        match device.as_str() {
            "vulkan" => probe_vulkan_vram_bytes(),
            "cuda" => probe_cuda_vram_bytes(),
            _ => None,
        }
    }
}

impl Default for DeviceManagerImpl {
    fn default() -> Self {
        Self::new(
            Box::new(crate::probe::RealSystemProbe),
            std::env::var("OCRX_MEMORY_GB")
                .ok()
                .and_then(|value| value.parse().ok()),
            ocrx_models::default_cache_root(),
        )
    }
}

impl DeviceManager for DeviceManagerImpl {
    fn snapshot(&self) -> ResourceSnapshot {
        self.snapshot_with_probe(self.probe.as_ref())
    }

    /// Selects a device: explicit requests must be available (no silent
    /// reselection); `auto` follows the frozen priority
    /// `metal > cuda > vulkan > cpu` over the availability flags.
    ///
    /// # Errors
    ///
    /// [`InferenceError::DeviceUnavailable`] for an unavailable explicit
    /// backend, [`InferenceError::InvalidTask`] for unknown names.
    fn select(&self, requested: &str) -> Result<BackendId, InferenceError> {
        let snapshot = self.snapshot();
        let priority = [
            ("metal", snapshot.metal_available),
            ("cuda", snapshot.cuda_available),
            ("vulkan", snapshot.vulkan_available),
            ("cpu", snapshot.cpu_available),
        ];
        match requested {
            "auto" => priority
                .into_iter()
                .find_map(|(device, available)| available.then(|| BackendId::new(device).expect("priority names are valid")))
                .ok_or(InferenceError::DeviceUnavailable("no available backend".into())),
            "metal" | "cuda" | "vulkan" | "cpu" => {
                let available = match requested {
                    "metal" => snapshot.metal_available,
                    "cuda" => snapshot.cuda_available,
                    "vulkan" => snapshot.vulkan_available,
                    _ => snapshot.cpu_available,
                };
                if available {
                    Ok(BackendId::new(requested).expect("explicit names are valid"))
                } else {
                    Err(InferenceError::DeviceUnavailable(requested.to_owned()))
                }
            }
            other => Err(InferenceError::InvalidTask(format!(
                "unknown device {other:?}"
            ))),
        }
    }

    /// Batch-depth tier selection (12 GiB → 16, otherwise 8; CPU or probe
    /// unavailable → 8). Re-called after a GPU→CPU fallback so the fallback
    /// depth lands on the floor.
    fn window_depth(&self, vram_bytes: Option<u64>) -> usize {
        ocrx_inference_api::window_depth_for_vram(vram_bytes)
    }

    /// Structured overview for `doctor` (§6.2): device facts plus the
    /// cache/install profile overview resolved through the models crate.
    fn diagnose(&self) -> DeviceDiagnostics {
        let snapshot = self.snapshot();
        let recommended_device = self
            .select("auto")
            .map(|backend| backend.to_string())
            .unwrap_or_else(|_| "cpu".to_owned());
        let recommended_profile = ocrx_models::recommend::recommend_profile(
            &["compact", "balanced", "quality", "reference"],
            snapshot.memory_gb,
        )
        .map(|profile| profile.to_string())
        .unwrap_or_else(|_| "compact".to_owned());
        let installed_profiles = ["compact", "balanced", "quality", "reference"]
            .into_iter()
            .filter(|profile| {
                ocrx_models::builtin::ovisocr2::ovisocr2_manifest(profile)
                    .ok()
                    .and_then(|manifest| ocrx_models::verify::is_installed(&self.cache_root, &manifest).ok())
                    .unwrap_or(false)
            })
            .map(ToOwned::to_owned)
            .collect();
        DeviceDiagnostics {
            platform: current_platform().to_owned(),
            backend: recommended_device_backend(&recommended_device),
            image_token_limit: 1024,
            memory_gb: snapshot.memory_gb,
            metal_available: snapshot.metal_available,
            cpu_available: snapshot.cpu_available,
            recommended_profile,
            recommended_device,
            cache_root: self.cache_root.to_string_lossy().into_owned(),
            installed_profiles,
            profiles: vec![
                "compact".to_owned(),
                "balanced".to_owned(),
                "quality".to_owned(),
                "reference".to_owned(),
            ],
        }
    }

    /// Backends compiled into this binary (CPU always; GPU per features).
    fn compiled_backends(&self) -> Vec<BackendId> {
        [Backend::Cpu, Backend::Metal, Backend::Cuda, Backend::Vulkan]
            .into_iter()
            .filter(|backend| backend.is_compiled())
            .map(|backend| BackendId::new(backend.as_str().to_ascii_lowercase()).expect("backend names are valid"))
            .collect()
    }
}

fn recommended_device_backend(recommended: &str) -> BackendId {
    BackendId::new(recommended).unwrap_or_else(|_| BackendId::new("cpu").expect("cpu is valid"))
}

/// Reports the current platform identifier for compatibility keys and the
/// doctor JSON report (single spelling source: `platform_paths::detect`).
pub(crate) fn current_platform() -> &'static str {
    // Platform detection logic stays in the CLI's platform_paths module
    // until T077; the inference side mirrors the same frozen strings.
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "unknown"
    }
}

#[must_use]
pub fn probe_vram_bytes(device: &str) -> Option<u64> {
    match device {
        "vulkan" => probe_vulkan_vram_bytes(),
        "cuda" => probe_cuda_vram_bytes(),
        _ => None,
    }
}

#[cfg(feature = "vulkan")]
fn probe_vulkan_vram_bytes() -> Option<u64> {
    use vulkano::instance::{Instance, InstanceCreateInfo};
    use vulkano::memory::MemoryHeapFlags;
    use vulkano::VulkanLibrary;

    let library = VulkanLibrary::new().ok()?;
    let instance = Instance::new(library, InstanceCreateInfo::default()).ok()?;
    instance
        .enumerate_physical_devices()
        .ok()?
        .filter_map(|physical| {
            let properties = physical.memory_properties();
            properties
                .memory_heaps
                .iter()
                .filter(|heap| heap.flags.contains(MemoryHeapFlags::DEVICE_LOCAL))
                .map(|heap| heap.size)
                .max()
        })
        .max()
}

#[cfg(not(feature = "vulkan"))]
fn probe_vulkan_vram_bytes() -> Option<u64> {
    None
}

#[must_use]
pub fn probe_cuda_vram_bytes() -> Option<u64> {
    if !cfg!(target_os = "windows") {
        return None;
    }
    let output = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let first = text.lines().next()?;
    let mb = first.trim().parse::<u64>().ok()?;
    Some(mb.saturating_mul(1024 * 1024))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::FakeSystemProbe;

    fn manager(probe: FakeSystemProbe) -> DeviceManagerImpl {
        DeviceManagerImpl::new(Box::new(probe), None, std::env::temp_dir())
    }

    #[allow(clippy::fn_params_excessive_bools)] // one bool per backend is the frozen contract
    fn probe(
        memory: u64,
        metal: bool,
        cuda: bool,
        vulkan: bool,
        cpu: bool,
    ) -> FakeSystemProbe {
        FakeSystemProbe {
            memory_gb: Some(memory),
            metal_available: metal,
            cuda_available: cuda,
            vulkan_available: vulkan,
            cpu_available: cpu,
        }
    }

    #[test]
    fn auto_selects_the_frozen_priority_with_compiled_intersection() {
        // Windows CPU-only build: metal is macOS-only, cuda/vulkan need
        // features — only cpu is both compiled and probed.
        let devices = manager(probe(16, true, true, true, true));
        assert_eq!(
            devices.select("auto").unwrap().as_str(),
            if cfg!(target_os = "macos") {
                "metal"
            } else {
                "cpu"
            }
        );
    }

    #[test]
    fn explicit_unavailable_devices_fail_loud() {
        let devices = manager(probe(16, false, false, false, true));
        if !Backend::Cuda.is_compiled() {
            assert!(matches!(
                devices.select("cuda"),
                Err(InferenceError::DeviceUnavailable(_))
            ));
        }
        assert!(matches!(
            devices.select("tpu"),
            Err(InferenceError::InvalidTask(_))
        ));
        assert_eq!(devices.select("cpu").unwrap().as_str(), "cpu");
    }

    #[test]
    fn compiled_backends_always_include_cpu() {
        let devices = manager(probe(16, false, false, false, true));
        let backends = devices.compiled_backends();
        assert!(backends.iter().any(|backend| backend.as_str() == "cpu"));
    }

    #[test]
    fn window_depth_tiers_by_probed_vram() {
        let devices = manager(probe(16, false, false, false, true));
        assert_eq!(devices.window_depth(None), 8);
        let twelve_gib = 12_u64 * 1024 * 1024 * 1024;
        assert_eq!(devices.window_depth(Some(twelve_gib)), 16);
        assert_eq!(devices.window_depth(Some(twelve_gib - 1)), 8);
    }

    #[test]
    fn diagnose_reports_the_structured_overview() {
        let devices = manager(probe(24, false, false, false, true));
        let diagnostics = devices.diagnose();
        assert_eq!(diagnostics.memory_gb, 24);
        assert_eq!(diagnostics.recommended_profile, "quality");
        assert_eq!(diagnostics.recommended_device, "cpu");
        assert!(diagnostics.installed_profiles.is_empty());
        assert_eq!(diagnostics.profiles.len(), 4);
    }
}
