//! `InferenceServiceImpl` (plan §5.5/§8.8): the fixed prepare order —
//! resolve catalog → select profile → verify binding → select device →
//! check assets → install if needed → create runtime (GPU→CPU fallback) →
//! create adapter/session — and the session that runs encode → generate →
//! decode per submit.

use ocrx_inference_api::{
    BackendHandle, BackendId, InferenceError, InferenceService, InferenceSession, ModelId,
    ModelIdentity, ModelDisplayMeta, PrepareRequest, PreparedInference, ProtocolAdapter,
    RuntimeTelemetry, StaticInferenceIdentity, StepProfile, TaskRequest, TaskResponse,
    DECODE_GENERATION,
};
use ocrx_inference_api::{DeviceManager, ModelManager};
use ocrx_models::ModelManagerImpl;
use ocrx_primitives::{CancellationToken, GenerationProgress};

use crate::binding;
use crate::device::DeviceManagerImpl;
use crate::registry;

/// Production service: catalog+assets via [`ModelManagerImpl`], devices via
/// [`DeviceManagerImpl`], backend loading via the injected factory.
pub struct InferenceServiceImpl {
    manager: ModelManagerImpl,
    devices: DeviceManagerImpl,
    factory: Box<dyn ocrx_inference_api::RuntimeFactory>,
}

impl InferenceServiceImpl {
    #[must_use]
    pub fn new(
        manager: ModelManagerImpl,
        devices: DeviceManagerImpl,
        factory: Box<dyn ocrx_inference_api::RuntimeFactory>,
    ) -> Self {
        Self {
            manager,
            devices,
            factory,
        }
    }

    /// The model manager (shared with the CLI's model/doctor commands).
    #[must_use]
    pub fn manager(&self) -> &ModelManagerImpl {
        &self.manager
    }

    #[must_use]
    pub fn manager_mut(&mut self) -> &mut ModelManagerImpl {
        &mut self.manager
    }

    #[must_use]
    pub fn devices(&self) -> &DeviceManagerImpl {
        &self.devices
    }

    /// Builds the model-side identity from a resolved manifest (assets
    /// sorted by role, per-asset pinned metadata).
    fn model_identity(
        model: &ModelId,
        manifest: &ocrx_inference_api::ModelManifest,
    ) -> Result<ModelIdentity, InferenceError> {
        let assets = manifest
            .assets
            .iter()
            .map(|asset| ocrx_inference_api::AssetIdentity {
                role: asset.role.clone(),
                sha256: asset.sha256.clone(),
                byte_len: asset.byte_len,
            })
            .collect();
        ModelIdentity::new(
            model.clone(),
            manifest.profile.clone(),
            manifest.source_revision.clone(),
            assets,
        )
        .map_err(|error| InferenceError::InvalidManifest(error.to_string()))
    }

    fn display_meta(manifest: &ocrx_inference_api::ModelManifest) -> ModelDisplayMeta {
        ModelDisplayMeta {
            display_name: manifest.display_name.clone(),
            family: manifest.family.clone(),
            quantization: manifest.quantization.clone(),
        }
    }
}

impl InferenceService for InferenceServiceImpl {
    /// Prepares one session (plan §5.5 fixed order; a GPU initialization
    /// failure at load time falls back to CPU with a warning and re-selects
    /// the floor batch depth; an explicitly unavailable device at select
    /// time fails loudly).
    ///
    /// # Errors
    ///
    /// Structured categories for whichever fixed step failed.
    fn prepare(
        &mut self,
        request: PrepareRequest,
        interaction: &mut dyn ocrx_inference_api::InstallInteraction,
    ) -> Result<PreparedInference, InferenceError> {
        // 1. Resolve the catalog (auto → concrete profile via the frozen
        // memory rule, shared with static_identity).
        let snapshot = self.devices.snapshot();
        let profile = self.manager.catalog().resolve_profile(
            &request.model,
            &request.requested_profile,
            snapshot.memory_gb,
        )?;
        // 2. Manifest + binding verification (registry-backed, same port
        // semantics as `model verify`).
        let manifest = self.manager.catalog().manifest(&request.model, &profile)?;
        binding::check_binding(&manifest, &registry::runtime_identity(), &registry::adapter_identity())?;
        // 3. Device selection: explicit-unavailable fails here, before any
        // asset work.
        let requested_backend = self.devices.select(&request.requested_device)?;
        // 4. Asset readiness: content check → install if missing per the
        // policy. Already-verified assets never download (idempotent).
        let installed = self.manager.verify_content_only(&manifest).is_ok();
        if !installed {
            self.manager.install(
                &request.model,
                &profile,
                request.install.offline,
                request.install.non_interactive,
                request.install.license_accepted,
                interaction,
            )?;
        }
        // 5. Load the runtime; GPU initialization failure falls back to CPU
        // with a warning and the re-selected floor depth.
        let mut warnings = Vec::new();
        let mut actual_backend = requested_backend.clone();
        let mut batch_depth = self.devices.window_depth(self.devices.vram_bytes(&requested_backend));
        let asset_paths = self.manager.required_asset_paths(&manifest)?;
        let path_of = |role: &str| -> Result<std::path::PathBuf, InferenceError> {
            asset_paths
                .iter()
                .find(|(asset_role, _)| asset_role.as_str() == role)
                .map(|(_, path)| path.clone())
                .ok_or_else(|| {
                    InferenceError::InvalidManifest(format!("missing asset role {role}"))
                })
        };
        let load = |backend: &BackendId, depth: usize| -> Result<BackendHandle, InferenceError> {
            self.factory.create(&ocrx_inference_api::RuntimeLoadRequest {
                model_path: path_of("model")?,
                mmproj_path: path_of("mmproj")?,
                device: backend.clone(),
                context_size: request.context_size,
                max_image_tokens: request.max_image_tokens,
                batch_depth: depth,
            })
        };
        let handle = match load(&actual_backend, batch_depth) {
            Ok(handle) => handle,
            Err(InferenceError::RuntimeInitialization(error))
                if actual_backend.as_str() != "cpu" =>
            {
                // GPU→CPU fallback: actual identity and report must say CPU,
                // and the batch depth re-selects to the floor (16→8).
                warnings.push(format!(
                    "gpu initialization failed ({error}); fell back to cpu"
                ));
                actual_backend = BackendId::new("cpu").expect("cpu is valid");
                batch_depth = self.devices.window_depth(None);
                load(&actual_backend, batch_depth)?
            }
            Err(error) => return Err(error),
        };
        // 6. Identity: actual backend + actual batch depth + platform
        // decode generation + display metadata from the resolved manifest.
        let identity = ocrx_inference_api::InferenceIdentity {
            model: Self::model_identity(&request.model, &manifest)?,
            runtime: handle.runtime.clone(),
            adapter: registry::adapter_identity(),
            backend: actual_backend,
            decode_generation: DECODE_GENERATION,
            batch_depth,
            display: Self::display_meta(&manifest),
        };
        let requested = requested_backend;
        let session = InferenceSessionImpl {
            backend: handle.backend,
            adapter: Box::new(ovis_adapter()),
            identity,
        };
        Ok(PreparedInference {
            session: Box::new(session),
            requested_backend: requested,
            warnings,
        })
    }

    /// Decode generation static query (D-15): the platform constant, never
    /// touching assets or runtimes.
    fn decode_generation(&self) -> u32 {
        DECODE_GENERATION
    }

    /// Read-only static identity (RBLK-4): catalog resolution (auto via the
    /// frozen memory rule — probing is the only device work here), built-in
    /// manifest per-asset metadata, and registry binding values. No asset
    /// file verification, no installation, no runtime creation, no device
    /// initialization.
    ///
    /// # Errors
    ///
    /// [`InferenceError::UnsupportedModel`]/[`InferenceError::UnsupportedProfile`].
    fn static_identity(
        &self,
        model: &ModelId,
        requested_profile: &str,
    ) -> Result<StaticInferenceIdentity, InferenceError> {
        // Resource probing serves only the auto recommendation (RBLK-4).
        let snapshot = self.devices.snapshot();
        let profile =
            self.manager
                .catalog()
                .resolve_profile(model, requested_profile, snapshot.memory_gb)?;
        let manifest = self.manager.catalog().manifest(model, &profile)?;
        Ok(StaticInferenceIdentity {
            model: Self::model_identity(model, &manifest)?,
            runtime: registry::runtime_identity(),
            adapter: registry::adapter_identity(),
            decode_generation: DECODE_GENERATION,
        })
    }
}

/// The single registered protocol adapter for prepare (composition root
/// wiring; the adapter crate knows nothing about sessions).
fn ovis_adapter() -> impl ProtocolAdapter {
    ocrx_ovisocr2::adapter::OvisProtocolAdapter::new()
}

/// One bound session (plan §3.3): created once per scan invocation, serving
/// many requests; each submit is encode → generate → decode. Telemetry and
/// step profile proxy the backend without double accumulation.
pub struct InferenceSessionImpl {
    backend: Box<dyn ocrx_inference_api::RuntimeBackend>,
    adapter: Box<dyn ProtocolAdapter>,
    identity: ocrx_inference_api::InferenceIdentity,
}

impl InferenceSession for InferenceSessionImpl {
    fn identity(&self) -> &ocrx_inference_api::InferenceIdentity {
        &self.identity
    }

    fn telemetry(&self) -> RuntimeTelemetry {
        self.backend.telemetry()
    }

    fn step_profile(&self) -> Option<StepProfile> {
        self.backend.step_profile()
    }

    /// Submits one task.
    ///
    /// # Errors
    ///
    /// Encode/decode categories from the adapter, execution/cancel from the
    /// backend.
    fn submit(
        &mut self,
        request: TaskRequest,
        cancellation: &CancellationToken,
        progress: &mut dyn FnMut(GenerationProgress),
    ) -> Result<TaskResponse, InferenceError> {
        let encoded = self.adapter.encode(request)?;
        let output = self.backend.generate(encoded, cancellation, progress)?;
        self.adapter.decode(output)
    }

    /// Window entry (P4.2 §⑤): encodes every request, submits the true
    /// multi-slot batch, and decodes slot-ordered results. A batch-level
    /// [`InferenceError::WindowFailed`] fills every entry; per-slot failures
    /// stay per-slot.
    fn submit_window(
        &mut self,
        requests: Vec<TaskRequest>,
        cancellation: &CancellationToken,
        progress: &mut dyn FnMut(usize, GenerationProgress),
    ) -> Vec<Result<TaskResponse, InferenceError>> {
        // Encode everything first: an invalid task means the window never
        // started, so every slot fails with that category (mirrors the
        // backend's whole-window behavior for pre-decode validation).
        let total = requests.len();
        let mut encoded = Vec::with_capacity(total);
        for request in requests {
            match self.adapter.encode(request) {
                Ok(task) => encoded.push(task),
                Err(error) => {
                    return std::iter::repeat_n(Err(error), total).collect();
                }
            }
        }
        let outputs = self
            .backend
            .generate_window(encoded, cancellation, progress);
        outputs
            .into_iter()
            .map(|result| result.and_then(|output| self.adapter.decode(output)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::FakeSystemProbe;
    use ocrx_inference_api::{
        DecodeEcho, EncodedTask, GenerationParams, ImageInput, InstallPolicy, ModelManifest,
        NativeFinishReason, NonInteractive, RuntimeBackend, TokenUsage,
    };
    use ocrx_runtime::FakeRuntimeBackend;
    use std::sync::{Arc, Mutex};

    /// A `RuntimeBackend` sharing one programmable fake behind a mutex, so
    /// tests can push responses after the session was created.
    struct SharedFake(Arc<Mutex<FakeRuntimeBackend>>);

    impl RuntimeBackend for SharedFake {
        fn generate(
            &mut self,
            task: EncodedTask,
            cancellation: &CancellationToken,
            progress: &mut dyn FnMut(GenerationProgress),
        ) -> Result<ocrx_inference_api::BackendOutput, InferenceError> {
            self.0.lock().unwrap().generate(task, cancellation, progress)
        }

        fn generate_window(
            &mut self,
            tasks: Vec<EncodedTask>,
            cancellation: &CancellationToken,
            progress: &mut dyn FnMut(usize, GenerationProgress),
        ) -> Vec<Result<ocrx_inference_api::BackendOutput, InferenceError>> {
            self.0
                .lock()
                .unwrap()
                .generate_window(tasks, cancellation, progress)
        }

        fn telemetry(&self) -> RuntimeTelemetry {
            self.0.lock().unwrap().telemetry()
        }

        fn step_profile(&self) -> Option<StepProfile> {
            self.0.lock().unwrap().step_profile()
        }
    }

    #[derive(Clone, Default)]
    struct FakeFactory {
        backend: Arc<Mutex<FakeRuntimeBackend>>,
    }

    impl ocrx_inference_api::RuntimeFactory for FakeFactory {
        fn create(
            &self,
            _request: &ocrx_inference_api::RuntimeLoadRequest,
        ) -> Result<BackendHandle, InferenceError> {
            Ok(BackendHandle {
                backend: Box::new(SharedFake(Arc::clone(&self.backend))),
                runtime: registry::runtime_identity(),
                backend_id: BackendId::new("cpu").unwrap(),
                image_token_limit: 1024,
            })
        }
    }

    fn service_with(
        backend: Arc<Mutex<FakeRuntimeBackend>>,
        probe: FakeSystemProbe,
    ) -> (InferenceServiceImpl, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "ocrx-inference-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        ));
        let manager = ModelManagerImpl::new(
            root.clone(),
            Box::new(registry::RegistryVerifier),
            Box::new(ocrx_models::FakeTransport::new()),
            None,
        );
        let devices = DeviceManagerImpl::new(Box::new(probe), None, root.clone());
        (
            InferenceServiceImpl::new(manager, devices, Box::new(FakeFactory { backend })),
            root,
        )
    }

    fn prepare_request() -> PrepareRequest {
        PrepareRequest {
            model: ModelId::new("ovisocr2").unwrap(),
            requested_profile: "compact".to_owned(),
            requested_device: "cpu".to_owned(),
            max_image_tokens: 1024,
            context_size: 20_480,
            install: InstallPolicy {
                offline: true,
                non_interactive: true,
                license_accepted: false,
            },
        }
    }

    fn task() -> TaskRequest {
        TaskRequest::new(
            "scan prompt",
            vec![ImageInput::new("img", 1, 1, vec![1, 2, 3]).unwrap()],
            GenerationParams {
                max_output_tokens: 64,
                temperature_milli: 0,
                thinking: false,
            },
        )
        .unwrap()
    }

    fn identity() -> ocrx_inference_api::InferenceIdentity {
        ocrx_inference_api::InferenceIdentity {
            model: ModelIdentity::new(
                ModelId::new("ovisocr2").unwrap(),
                ocrx_inference_api::ProfileId::new("compact").unwrap(),
                "rev",
                vec![],
            )
            .unwrap(),
            runtime: registry::runtime_identity(),
            adapter: registry::adapter_identity(),
            backend: BackendId::new("cpu").unwrap(),
            decode_generation: DECODE_GENERATION,
            batch_depth: 8,
            display: ModelDisplayMeta {
                display_name: "OvisOCR2".into(),
                family: "Ovis".into(),
                quantization: "Q4_K_M".into(),
            },
        }
    }

    fn session_with(backend: FakeRuntimeBackend) -> InferenceSessionImpl {
        InferenceSessionImpl {
            backend: Box::new(backend),
            adapter: Box::new(ovis_adapter()),
            identity: identity(),
        }
    }

    #[test]
    fn prepare_offline_without_assets_fails_before_creating_a_session() {
        let backend = Arc::new(Mutex::new(FakeRuntimeBackend::new()));
        let (mut service, root) = service_with(backend, FakeSystemProbe::default());
        let mut interaction = NonInteractive;
        let error = match service.prepare(prepare_request(), &mut interaction) {
            Err(error) => error,
            Ok(_) => panic!("prepare must fail without assets"),
        };
        assert!(matches!(
            error,
            InferenceError::Offline | InferenceError::MissingAsset(_)
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn prepare_fails_loudly_for_unknown_models() {
        let backend = Arc::new(Mutex::new(FakeRuntimeBackend::new()));
        let (mut service, root) = service_with(backend, FakeSystemProbe::default());
        let mut request = prepare_request();
        request.model = ModelId::new("qwen3.8").unwrap();
        let mut interaction = NonInteractive;
        assert!(
            service.prepare(request, &mut interaction).is_err(),
            "expected unsupported model to be rejected"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn session_serves_many_submits_with_cumulative_telemetry() {
        let mut backend = FakeRuntimeBackend::new();
        backend.push_eos("first");
        backend.push_eos("second");
        let mut session = session_with(backend);
        let first = session
            .submit(task(), &CancellationToken::new(), &mut |_| {})
            .unwrap();
        let second = session
            .submit(task(), &CancellationToken::new(), &mut |_| {})
            .unwrap();
        assert_eq!(first.text, "first");
        assert_eq!(second.text, "second");
        assert_eq!(session.telemetry().generated_tokens, 2);
        assert_eq!(session.identity().backend.as_str(), "cpu");
        assert_eq!(session.identity().batch_depth, 8);
        assert_eq!(session.identity().decode_generation, DECODE_GENERATION);
        assert_eq!(session.identity().display.display_name, "OvisOCR2");
    }

    #[test]
    fn submit_window_maps_batch_failures_to_every_entry() {
        let mut backend = FakeRuntimeBackend::new();
        backend.fail_windows_with(InferenceError::WindowFailed("overflow".into()));
        let mut session = session_with(backend);
        let results = session.submit_window(
            vec![task(), task(), task()],
            &CancellationToken::new(),
            &mut |_, _| {},
        );
        assert_eq!(results.len(), 3);
        assert!(results
            .iter()
            .all(|result| matches!(result, Err(InferenceError::WindowFailed(_)))));
    }

    #[test]
    fn session_decode_echo_passes_the_backend_receipt_through() {
        let mut backend = FakeRuntimeBackend::new();
        backend.push_eos("x");
        backend.set_divergent_echo(DecodeEcho {
            image_tokens: Some(2048),
            sampling: None,
        });
        let mut session = session_with(backend);
        let response = session
            .submit(task(), &CancellationToken::new(), &mut |_| {})
            .unwrap();
        assert_eq!(response.decode_echo.image_tokens, Some(2048));
    }

    #[test]
    fn session_preserves_per_slot_failures_in_windows() {
        let mut backend = FakeRuntimeBackend::new();
        backend.push_eos("ok");
        backend.push_error(InferenceError::BackendExecution("slot fail".into()));
        let mut session = session_with(backend);
        let results = session.submit_window(
            vec![task(), task()],
            &CancellationToken::new(),
            &mut |_, _| {},
        );
        assert!(results[0].is_ok());
        assert!(matches!(results[1], Err(InferenceError::BackendExecution(_))));
    }

    #[test]
    fn static_identity_resolves_auto_and_carries_the_registry_values() {
        let backend = Arc::new(Mutex::new(FakeRuntimeBackend::new()));
        let (service, root) = service_with(backend, FakeSystemProbe::default());
        let statik = service
            .static_identity(&ModelId::new("ovisocr2").unwrap(), "auto")
            .unwrap();
        // The fake probe defaults to 16 GiB -> balanced per the frozen ladder.
        assert_eq!(statik.model.profile.as_str(), "balanced");
        assert_eq!(statik.runtime.runtime.as_str(), "llama.cpp");
        assert_eq!(statik.runtime.wrapper_version, "0.1.161");
        assert_eq!(statik.adapter.adapter.as_str(), "ovisocr2");
        assert_eq!(statik.decode_generation, DECODE_GENERATION);
        // No backend/batch depth/display in the static subset (RBLK-7).
        let json = serde_json::to_string(&statik).unwrap();
        assert!(!json.contains("\"backend\""));
        assert!(!json.contains("batch_depth"));
        assert!(!json.contains("display"));
        assert_eq!(service.decode_generation(), DECODE_GENERATION);
        let _ = std::fs::remove_dir_all(root);
    }

    // Keep the unused-import set minimal while later tasks land.
    #[allow(dead_code)]
    fn _unused(_: ModelManifest, _: NativeFinishReason, _: TokenUsage) {}
}
