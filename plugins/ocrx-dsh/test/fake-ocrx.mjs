#!/usr/bin/env node
// Scenario-driven fake ocrx binary (plan D-9): the plugin spawns a REAL
// executable (the platform wrapper) and the scenario comes from the
// OCRX_FAKE_SCENARIO environment variable, so argv stays clean for argv
// mapping assertions. Scenario enumeration mirrors plan §14.1.1 test items.
import { sleep } from 'node:timers/promises';

const scenario = process.env.OCRX_FAKE_SCENARIO ?? 'usage_error';

const argv = process.argv.slice(2);

const failureObject = (code, kind, message, cause = null, suggestion = null) =>
  JSON.stringify({ schema_version: 1, code, kind, message, cause, suggestion });

const v3Result = (overrides = {}) =>
  JSON.stringify({
    schema_version: 3,
    job_id: 'scan',
    status: overrides.status ?? 'success',
    input: overrides.input ?? 'C:/docs/sample.pdf',
    markdown_path: overrides.markdown_path ?? 'C:/out/sample.md',
    output_dir: overrides.output_dir ?? 'C:/out/sample',
    required_quality_tier: 'l2',
    document_type: 'scanned',
    ocr_model_loaded: overrides.ocr_model_loaded ?? true,
    model: { id: 'ovisocr3', name: 'OvisOCR3', family: 'Ovis', profile: 'quality', revision: 'rev', mmproj_revision: 'rev2', adapter: 'ovisocr3', adapter_version: '0.1.0', prompt_schema_version: 1, runtime: 'llama.cpp@0.1.161', quantization: 'Q8_0' },
    backend: 'cpu',
    reread: null,
    pages: {
      total: overrides.total ?? 42,
      completed: overrides.completed ?? 42,
      failed: overrides.failed ?? 0,
      truncated: 0,
      degenerated: 0,
      think_blocks_stripped_total: 0,
    },
    routing_counts: { native_text: 0, ocr: 42, empty: 0, fallback: 0 },
    images: { directory: 'C:/out/sample/images', count: 3 },
    elapsed_ms: overrides.elapsed_ms ?? 186400,
    warnings: overrides.warnings ?? ['one-warning'],
    diagnostics: [],
    pdf_routing: [],
    routing_metadata: [],
  });

const aggregate = (documents, extra = {}) =>
  JSON.stringify({
    schema_version: 1,
    mode: 'multi',
    inputs: documents.map((document) => document.input),
    output_root: extra.output_root ?? 'C:/out',
    documents,
    elapsed_ms: extra.elapsed_ms ?? 1000,
  });

const installSummary = (overrides = {}) =>
  JSON.stringify({
    model: overrides.model ?? 'ovisocr3',
    profile: overrides.profile ?? 'quality',
    revision: overrides.revision ?? '7be644a3',
    size_bytes: 1016830848,
    elapsed_ms: overrides.elapsed_ms ?? 654321,
    stage: 'done',
    download: {
      downloaded: overrides.downloaded ?? true,
      per_asset: overrides.downloaded === false
        ? []
        : [
            { role: 'model', bytes: 811843712, elapsed_ms: 540000, avg_rate_bps: 1503400.2 },
            { role: 'mmproj', bytes: 204987136, elapsed_ms: 114321, avg_rate_bps: 1793070.5 },
          ],
      total_bytes: overrides.downloaded === false ? 0 : 1016830848,
      avg_rate_bps: overrides.downloaded === false ? 0 : 1555483.7,
    },
    verify: {
      per_asset: [
        { role: 'model', sha256_ok: true, size_ok: true },
        { role: 'mmproj', sha256_ok: true, size_ok: true },
      ],
    },
  });

const installPlan = (overrides = {}) =>
  JSON.stringify({
    schema_version: 1,
    action: 'recommend',
    model: overrides.model ?? 'ovisocr3',
    profile: overrides.profile ?? 'quality',
    default_model: 'ovisocr3',
    revision: overrides.revision ?? '7be644a312005c3a55e2c826aee985a5c93d06b2',
    size_bytes: overrides.size_bytes ?? 1016830848,
    target_dir: overrides.target_dir ?? 'C:/users/x/AppData/Local/ocrx/models/ovisocr3/7be644a3',
    assets: [
      { role: 'model', path: 'C:/users/x/AppData/Local/ocrx/models/ovisocr3/7be644a3/quality/model.gguf', size_bytes: 811843712 },
      { role: 'mmproj', path: 'C:/users/x/AppData/Local/ocrx/models/ovisocr3/7be644a3/mmproj.gguf', size_bytes: 204987136 },
    ],
    installed: false,
    cache_root: 'C:/users/x/AppData/Local/ocrx/models',
  });

const doctorJson = () =>
  JSON.stringify({
    schema_version: 2,
    version: '0.2.0',
    platform: 'windows',
    model: 'ovisocr3',
    backend: 'Cpu',
    image_token_limit: 2048,
    memory_gb: 16,
    metal_available: false,
    cpu_available: true,
    cache_root: 'C:/users/x/AppData/Local/ocrx/models',
    installed_profiles: [],
    recommended_profile: 'quality',
    recommended_device: 'cpu',
    skill_installed: false,
    profiles: ['quality'],
  });

const scenarios = {
  usage_error: () => {
    process.stderr.write('error: unsupported or incomplete command: nope\n');
    process.exit(2);
  },
  // --- scan -------------------------------------------------------------
  scan_single_v3_ok: () => {
    process.stdout.write(`${v3Result()}\n`);
    process.exit(0);
  },
  scan_single_v3_partial: () => {
    process.stdout.write(`${v3Result({ status: 'partial', failed: 2, completed: 40 })}\n`);
    process.exit(10);
  },
  scan_single_fail_model_missing: () => {
    process.stdout.write(`${failureObject(4, 'model', 'model assets missing', 'offline', null)}\n`);
    process.exit(4);
  },
  scan_single_fail_input: () => {
    process.stdout.write(`${failureObject(3, 'input', 'input file missing', null, null)}\n`);
    process.exit(3);
  },
  scan_batch_ok: () => {
    process.stdout.write(`${aggregate([
      { input: 'C:/docs/a.pdf', output_dir: 'C:/out/a', status: 'success', exit_code: 0, elapsed_ms: 100 },
      { input: 'C:/docs/b.pdf', output_dir: 'C:/out/b', status: 'success', exit_code: 0, elapsed_ms: 100 },
    ])}\n`);
    process.exit(0);
  },
  scan_batch_mixed: () => {
    // model_missing first, input second (plan §6 阶段 4 归一化测试)。
    process.stderr.write(`${failureObject(4, 'model', 'model assets missing', 'offline', null)}\n`);
    process.stderr.write('[doc 2/2 C:/docs/b.pdf]\n');
    process.stderr.write(`${failureObject(3, 'input', 'input file missing', null, null)}\n`);
    process.stdout.write(`${aggregate([
      { input: 'C:/docs/a.pdf', output_dir: 'C:/out/a', status: 'failed', exit_code: 4, elapsed_ms: 0 },
      { input: 'C:/docs/b.pdf', output_dir: 'C:/out/b', status: 'failed', exit_code: 3, elapsed_ms: 0 },
    ])}\n`);
    process.exit(3);
  },
  scan_batch_fifo_same_code_different_cause: () => {
    // Two documents fail with the SAME code/kind but different causes —
    // FIFO pairing must not guess by kind (plan §6 阶段 4 冻结规则).
    process.stderr.write(`${failureObject(4, 'model', 'model assets missing', 'offline', null)}\n`);
    process.stderr.write(`${failureObject(4, 'model', 'runtime init failed', 'runtime_initialization', null)}\n`);
    process.stdout.write(`${aggregate([
      { input: 'C:/docs/a.pdf', output_dir: 'C:/out/a', status: 'failed', exit_code: 4, elapsed_ms: 0 },
      { input: 'C:/docs/b.pdf', output_dir: 'C:/out/b', status: 'failed', exit_code: 4, elapsed_ms: 0 },
    ])}\n`);
    process.exit(3);
  },
  scan_batch_missing_object: () => {
    // Two failed documents, only ONE failure object (M < N: the tail was
    // truncated) — the unpaired document falls back to code default.
    process.stderr.write(`${failureObject(4, 'model', 'model assets missing', 'offline', null)}\n`);
    process.stdout.write(`${aggregate([
      { input: 'C:/docs/a.pdf', output_dir: 'C:/out/a', status: 'failed', exit_code: 4, elapsed_ms: 0 },
      { input: 'C:/docs/b.pdf', output_dir: 'C:/out/b', status: 'failed', exit_code: 3, elapsed_ms: 0 },
    ])}\n`);
    process.exit(3);
  },
  scan_batch_extra_object: () => {
    // One failed document but TWO failure objects (M > N: the extra belongs
    // to a 130 document we cannot attribute) — degrade, never mis-attribute.
    process.stderr.write(`${failureObject(3, 'input', 'input file missing', null, null)}\n`);
    process.stderr.write(`${failureObject(4, 'model', 'model assets missing', 'offline', null)}\n`);
    process.stdout.write(`${aggregate([
      { input: 'C:/docs/a.pdf', output_dir: 'C:/out/a', status: 'failed', exit_code: 3, elapsed_ms: 0 },
      { input: 'C:/docs/b.pdf', output_dir: 'C:/out/b', status: 'cancelled', exit_code: 130, elapsed_ms: 0 },
    ])}\n`);
    process.exit(3);
  },
  scan_batch_cancel_then_fail: () => {
    // [130, failure]: the cancelled document has NO object; the failure doc
    // has exactly one (M == N) — the cancel must stay `interrupted` and the
    // failure reason must NOT shift onto it.
    process.stderr.write(`${failureObject(3, 'input', 'input file missing', null, null)}\n`);
    process.stdout.write(`${aggregate([
      { input: 'C:/docs/a.pdf', output_dir: 'C:/out/a', status: 'cancelled', exit_code: 130, elapsed_ms: 0 },
      { input: 'C:/docs/b.pdf', output_dir: 'C:/out/b', status: 'failed', exit_code: 3, elapsed_ms: 0 },
    ])}\n`);
    process.exit(3);
  },
  scan_exit6_device_object: () => {
    process.stdout.write(
      `${failureObject(6, 'runtime', 'The requested device is unavailable on this build', 'device', null)}\n`,
    );
    process.exit(6);
  },
  scan_exit6_text_only: () => {
    process.stderr.write('error: runtime initialization failed: memory below floor\n');
    process.exit(6);
  },
  scan_exit3_pdf_native: () => {
    process.stdout.write(`${failureObject(3, 'pdf_native', 'unreliable text layer', null, null)}\n`);
    process.exit(3);
  },
  scan_exit3_pdf_missing: () => {
    process.stdout.write(`${failureObject(3, 'pdf', 'PDFium library missing', null, null)}\n`);
    process.exit(3);
  },
  scan_exit5_output: () => {
    process.stdout.write(`${failureObject(5, 'output', 'result.json commit failed', null, null)}\n`);
    process.exit(5);
  },
  scan_exit9_output_policy: () => {
    process.stdout.write(
      `${failureObject(9, 'output', 'output directory refused by the output policy', 'scan-summary.json already exists; use --resume or --overwrite', 'choose a dedicated output directory')}\n`,
    );
    process.exit(9);
  },
  scan_exit10_partial: () => {
    process.stderr.write(`${failureObject(3, 'input', 'input file missing', null, null)}\n`);
    process.stdout.write(`${aggregate([
      { input: 'C:/docs/a.pdf', output_dir: 'C:/out/a', status: 'success', exit_code: 0, elapsed_ms: 100 },
      { input: 'C:/docs/b.pdf', output_dir: 'C:/out/b', status: 'failed', exit_code: 3, elapsed_ms: 0 },
    ])}\n`);
    process.exit(10);
  },
  scan_exit2_usage: () => {
    process.stderr.write('error: unsupported or incomplete command: scan\n');
    process.exit(2);
  },
  scan_exit130_console: () => {
    process.exit(130);
  },
  sleep: async () => {
    const ms = Number(process.env.OCRX_FAKE_SLEEP_MS ?? '60000');
    await sleep(ms);
    process.exit(0);
  },
  // --- doctor / model ----------------------------------------------------
  doctor_json: () => {
    process.stdout.write(`${doctorJson()}\n`);
    process.exit(0);
  },
  doctor_json_old: () => {
    // Pre-C3 binary: no version field.
    const parsed = JSON.parse(doctorJson());
    delete parsed.version;
    process.stdout.write(`${JSON.stringify(parsed)}\n`);
    process.exit(0);
  },
  model_list: () => {
    process.stdout.write('model=ovisocr2\nmodel=ovisocr3\n');
    process.exit(0);
  },
  model_info: () => {
    process.stdout.write('model=ovisocr3 profiles=quality installed=\n');
    process.exit(0);
  },
  model_recommend_plan: () => {
    process.stdout.write(`${installPlan()}\n`);
    process.exit(0);
  },
  model_recommend_plan_ovisocr2: () => {
    // Low-memory controlled scenario: the no-flag recommend crosses to
    // ovisocr2/balanced (readiness matrix fixture).
    process.stdout.write(`${installPlan({ model: 'ovisocr2', profile: 'balanced', revision: '8a22290a0c42dbcf84739d6d4c2763f877494ae0', size_bytes: 641_000_000 })}\n`);
    process.exit(0);
  },
  model_recommend_text: () => {
    process.stdout.write('model=ovisocr3 profile=quality memory_gb=16 device=cpu\n');
    process.exit(0);
  },
  model_recommend_old_binary: () => {
    // Pre-C2 binary: --report is an unsupported model flag (exit 2).
    process.stderr.write('error: unsupported model flag: --report\n');
    process.exit(2);
  },
  model_verify_ok: () => {
    process.stdout.write('model=ovisocr3 profile=quality revision=7be644a3 status=installed\n');
    process.exit(0);
  },
  model_verify_fail: () => {
    process.stderr.write('model verification failed: ...\n');
    process.exit(4);
  },
  // --- install -----------------------------------------------------------
  install_summary: () => {
    process.stderr.write('install lock-acquired pid=42\n');
    process.stderr.write('install download-start role=model url=https://x bytes=811843712 attempt=1\n');
    process.stdout.write('model=ovisocr3 profile=quality revision=7be644a3 status=installed\n');
    process.stdout.write(`${installSummary()}\n`);
    process.exit(0);
  },
  install_idempotent_summary: () => {
    process.stdout.write('model=ovisocr3 profile=quality revision=7be644a3 status=installed\n');
    process.stdout.write(`${installSummary({ downloaded: false })}\n`);
    process.exit(0);
  },
  install_fail_transport: () => {
    process.stderr.write('install download-start role=model url=https://x bytes=811843712 attempt=3\n');
    process.stdout.write(`${failureObject(4, 'transport', 'download failed', 'connection reset', null)}\n`);
    process.exit(4);
  },
  install_fail_hash: () => {
    process.stdout.write(`${failureObject(4, 'hash', 'asset model hash mismatch', null, null)}\n`);
    process.exit(4);
  },
  install_fail_lock: () => {
    process.stdout.write(`${failureObject(4, 'lock', 'another install holds the lock (holder pid 4242)', null, null)}\n`);
    process.exit(4);
  },
  install_fail_offline_text: () => {
    // Pre-C2 binary face: bare stderr text, exit 5.
    process.stderr.write('model install blocked: offline mode\n');
    process.exit(5);
  },
  install_fail_disk_text: () => {
    process.stderr.write('model install blocked: insufficient disk\n');
    process.exit(5);
  },
  install_fail_transport_text: () => {
    // Pre-C2 binary face: generic failure with one stderr line.
    process.stderr.write('model install failed: Transport("connection reset")\n');
    process.exit(4);
  },
  // --- launcher ----------------------------------------------------------
  launcher_fail: () => {
    // Windows loader failure (0xC0000135 STATUS_DLL_NOT_FOUND) with no
    // output at all.
    process.exit(3221225781);
  },
  silent_crash: () => {
    process.exit(1);
  },
};

const handler = scenarios[scenario];
if (!handler) {
  process.stderr.write(`fake-ocrx: unknown scenario ${scenario}\n`);
  process.exit(2);
}
await handler();
