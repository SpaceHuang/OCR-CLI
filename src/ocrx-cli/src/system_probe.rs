//! Injectable system probe for hardware/resource detection.
//!
//! `ResourceSnapshot` construction depends on memory size and GPU backend
//! availability, which are machine-specific. The [`SystemProbe`] trait
//! abstracts that detection so recommendation logic can be tested against a
//! fixed fake instead of the host hardware (W1.2). The real implementation
//! lives in [`RealSystemProbe`]; tests inject a fake.

/// Hardware/resource facts needed to build a [`crate::ResourceSnapshot`].
///
/// `cpu_available` defaults to `true`: the CPU backend is always compilable
/// and present, so only an explicit probe override would disable it.
pub trait SystemProbe {
    /// Total physical memory in GiB, or `None` when the host refuses to
    /// report it (the caller falls back to a sane default).
    fn detected_memory_gb(&self) -> Option<u64>;
    fn metal_available(&self) -> bool;
    fn cuda_available(&self) -> bool;
    fn vulkan_available(&self) -> bool;
    fn cpu_available(&self) -> bool {
        true
    }
}

/// Probe that reads the real host. Safe-only: memory is read via the
/// platform's standard introspection command and GPU backends are detected by
/// the presence of their driver/loader DLL, so no `unsafe` FFI is required
/// (the workspace forbids `unsafe_code`).
#[derive(Debug, Default, Clone, Copy)]
pub struct RealSystemProbe;

impl SystemProbe for RealSystemProbe {
    fn detected_memory_gb(&self) -> Option<u64> {
        if cfg!(target_os = "macos") {
            detect_memory_macos()
        } else if cfg!(target_os = "windows") {
            detect_memory_windows()
        } else {
            None
        }
    }

    fn metal_available(&self) -> bool {
        cfg!(target_os = "macos")
    }

    fn cuda_available(&self) -> bool {
        cfg!(target_os = "windows") && system_dll_present("nvcuda.dll")
    }

    fn vulkan_available(&self) -> bool {
        if !cfg!(target_os = "windows") {
            return false;
        }
        #[cfg(feature = "vulkan")]
        {
            detect_vulkan_device()
        }
        #[cfg(not(feature = "vulkan"))]
        {
            system_dll_present("vulkan-1.dll")
        }
    }

    fn cpu_available(&self) -> bool {
        true
    }
}

/// macOS total memory via `sysctl hw.memsize`.
fn detect_memory_macos() -> Option<u64> {
    std::process::Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|bytes| bytes / 1024 / 1024 / 1024)
}

/// Windows total memory via the CIM provider (`Win32_ComputerSystem`), which
/// internally calls `GlobalMemoryStatusEx`. Best-effort: any failure returns
/// `None` and the caller falls back to `OCRX_MEMORY_GB` or the default, so a
/// restricted PowerShell or missing provider never blocks the CLI.
fn detect_memory_windows() -> Option<u64> {
    std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory",
        ])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|bytes| bytes / 1024 / 1024 / 1024)
}

/// Reports whether a named loader/driver DLL is present in `%SystemRoot%\System32`.
///
/// This is a v0.1 availability proxy: a full Vulkan instance or CUDA device-count
/// probe needs `unsafe` FFI (forbidden by the workspace lint) and is deferred.
/// The presence of the loader DLL is a sufficient signal that the backend can be
/// initialized, and the injectable trait lets tests assert the selection logic
/// without depending on the real probe.
fn system_dll_present(name: &str) -> bool {
    std::env::var_os("SystemRoot")
        .map(|root| std::path::Path::new(&root).join("System32").join(name))
        .map(|path| path.is_file())
        .unwrap_or(false)
}

/// Real Vulkan instance/device probe used when the `vulkan` cargo feature is
/// compiled in. The workspace forbids `unsafe`, so this uses `vulkano`, a safe
/// Vulkan wrapper, to create an instance and check for at least one physical
/// device. The instance is dropped by `vulkano` at scope exit.
#[cfg(feature = "vulkan")]
fn detect_vulkan_device() -> bool {
    use vulkano::VulkanLibrary;
    use vulkano::instance::{Instance, InstanceCreateInfo};

    let Ok(library) = VulkanLibrary::new() else {
        return false;
    };
    let Ok(instance) = Instance::new(library, InstanceCreateInfo::default()) else {
        return false;
    };
    instance
        .enumerate_physical_devices()
        .map(|mut devices| devices.next().is_some())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake probe returning fixed values, for snapshot/recommendation tests.
    #[allow(clippy::struct_excessive_bools)] // mirrors the one-bool-per-backend contract
    #[derive(Debug, Clone, Copy)]
    pub struct FakeSystemProbe {
        pub memory_gb: Option<u64>,
        pub metal_available: bool,
        pub cuda_available: bool,
        pub vulkan_available: bool,
        pub cpu_available: bool,
    }

    impl Default for FakeSystemProbe {
        fn default() -> Self {
            Self {
                memory_gb: Some(16),
                metal_available: false,
                cuda_available: false,
                vulkan_available: false,
                cpu_available: true,
            }
        }
    }

    impl SystemProbe for FakeSystemProbe {
        fn detected_memory_gb(&self) -> Option<u64> {
            self.memory_gb
        }
        fn metal_available(&self) -> bool {
            self.metal_available
        }
        fn cuda_available(&self) -> bool {
            self.cuda_available
        }
        fn vulkan_available(&self) -> bool {
            self.vulkan_available
        }
        fn cpu_available(&self) -> bool {
            self.cpu_available
        }
    }

    #[test]
    fn real_probe_reports_cpu_available_on_every_platform() {
        // The CPU backend is unconditionally compilable; the real probe must
        // never report it unavailable, or `auto` could fail to select anything.
        assert!(RealSystemProbe.cpu_available());
    }

    #[test]
    fn fake_probe_round_trips_configured_values() {
        let probe = FakeSystemProbe {
            memory_gb: Some(24),
            cuda_available: true,
            ..FakeSystemProbe::default()
        };
        assert_eq!(probe.detected_memory_gb(), Some(24));
        assert!(probe.cuda_available());
        assert!(!probe.vulkan_available());
    }
}
