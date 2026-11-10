//! P4.1.5 de-risk spikes (plan §7.3): batch-divergence quantification (Spike
//! A) and multi-sequence decode combination verification (Spike B).
//!
//! Env-gated (never runs in `cargo test` by default): set `OCRX_E2E_MODEL=1`.
//! Requires the installed reference profile in the default model cache, the
//! vendored PDFium library, and the golden-set composite at
//! `assets/regression/ovisocr2-draft-v1/inputs/source.pdf` (not redistributed).
//!
//! Spike B (feasibility): exercises the binding-level multi-sequence
//! combination — per-sequence `eval_chunks` with `seq_id`, shared-context
//! decode via one `LlamaBatch` with per-sequence logits, `get_logits_ith`,
//! and per-sequence `kv_cache_seq_rm` slot recycling.
//!
//! Spike A (divergence data): the same real pages are decoded single-stream
//! and batched (N=2, N=4); the record reports, per page and batch depth, the
//! first diverging token index and the text similarity of the batched output
//! against the single-stream output — the measured basis for the §2.3 第二档
//! threshold that P4.2 must freeze.
//!
//! The JSON record goes to `OCRX_SPIKE_OUT` (a file path) or a temp file
//! whose path is printed.

use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::{
    LlamaModel,
    params::{LlamaModelParams, LlamaSplitMode},
};
use llama_cpp_2::mtmd::{MtmdBitmap, MtmdContext, MtmdContextParams, MtmdInputText};
use llama_cpp_2::token::LlamaToken;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::time::Instant;

const TOKEN_CAP: usize = 96;
const PAGE_COUNT: usize = 6;
const DPI: u16 = 150;
const BATCH_DEPTHS: [usize; 2] = [2, 4];

fn skip_reason() -> Option<&'static str> {
    if std::env::var_os("OCRX_E2E_MODEL").is_none() {
        return Some("set OCRX_E2E_MODEL=1 to run the P4.1.5 spikes");
    }
    None
}

fn resolve_model_paths() -> Result<(PathBuf, PathBuf), String> {
    let cache = std::env::var_os("OCRX_MODEL_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(ocrx_models::default_cache_root);
    let _manifest =
        ocrx_models::community_manifest("reference").map_err(|error| format!("{error:?}"))?;
    ocrx_models::cache_paths(cache, ocrx_models::COMMUNITY_REVISION, "reference")
        .map_err(|error| format!("{error:?}"))
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn pdfium_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("OCRX_PDFIUM_LIB_DIR") {
        return PathBuf::from(dir);
    }
    let vendored = repo_root().join("vendor/pdfium/bin");
    if vendored.join("pdfium.dll").is_file() || vendored.join("libpdfium.dylib").is_file() {
        return vendored;
    }
    // Worktrees without the gitignored vendored dll fall back to the main
    // checkout next to them.
    repo_root().join("../OCR-CLI/vendor/pdfium/bin")
}

fn render_pages() -> Vec<(u32, u32, Vec<u8>)> {
    let pdfium_dir = pdfium_dir();
    let source = repo_root().join("assets/regression/ovisocr2-draft-v1/inputs/source.pdf");
    let renderer = ocrx_pdf::PdfiumRenderer::new(pdfium_dir);
    (0..PAGE_COUNT)
        .map(|index| {
            let (image, _, _) = renderer
                .render_page_rgb8_with_audit(&source, index, ocrx_pdf::RenderOptions { dpi: DPI })
                .expect("golden page must render");
            (image.width(), image.height(), image.into_raw())
        })
        .collect()
}

/// Encodes one page + the prompt for `seq_id`; returns the next free position
/// for that sequence. `eval_chunks(..., logits_last=true)` leaves the logits
/// row of the last image token readable at the returned position - 1.
fn encode_sequence(
    mtmd: &mut MtmdContext,
    context: &mut LlamaContext<'_>,
    width: u32,
    height: u32,
    rgb: &[u8],
    seq_id: i32,
) -> Result<i32, String> {
    let bitmap =
        MtmdBitmap::from_image_data(width, height, rgb).map_err(|error| error.to_string())?;
    let chunks = mtmd
        .tokenize(
            MtmdInputText {
                text: "\nExtract all readable content from the image.".to_owned(),
                add_special: true,
                parse_special: true,
            },
            &[&bitmap],
        )
        .map_err(|error| error.to_string())?;
    chunks
        .eval_chunks(mtmd, context, 0, seq_id, 512, true)
        .map_err(|error| error.to_string())
}

/// Greedy argmax over one logits row.
fn argmax_token(logits: &[f32]) -> i32 {
    logits
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| left.total_cmp(right))
        .map(|(id, _)| id)
        .expect("logits row is non-empty") as i32
}

/// Seeds a sequence from the single initialized logits row that
/// `eval_chunks(..., logits_last = true)` leaves in the context buffer. Must
/// run immediately after that sequence's encode (the next eval overwrites).
fn seed_from_last_eval(context: &LlamaContext<'_>) -> i32 {
    argmax_token(context.get_logits())
}

/// Greedy-decodes all live sequences from one shared batch until EOS or the
/// token cap. `last` holds each sequence's next input token (already sampled
/// from its encoded tail).
fn decode_shared(
    model: &LlamaModel,
    context: &mut LlamaContext<'_>,
    last: &mut [LlamaToken],
    positions: &mut [i32],
) -> Vec<Vec<i32>> {
    let mut finished = vec![false; last.len()];
    let mut streams: Vec<Vec<i32>> = vec![Vec::new(); last.len()];
    for _step in 0..TOKEN_CAP {
        if finished.iter().all(|f| *f) {
            break;
        }
        let mut batch = LlamaBatch::new(last.len(), last.len() as i32);
        let mut logits_index = Vec::new();
        for (sequence, token) in last.iter().enumerate() {
            if finished[sequence] {
                continue;
            }
            let index = batch.n_tokens();
            batch
                .add(*token, positions[sequence], &[sequence as i32], true)
                .expect("batch has room for every live sequence");
            logits_index.push((sequence, index));
            positions[sequence] += 1;
        }
        context
            .decode(&mut batch)
            .expect("shared decode succeeds for well-formed batches");
        for (sequence, index) in logits_index {
            let best = argmax_token(context.get_logits_ith(index));
            let token = LlamaToken::new(best);
            if model.vocab().is_eog(token) {
                finished[sequence] = true;
            } else {
                streams[sequence].push(best);
                last[sequence] = token;
            }
        }
    }
    streams
}

fn to_text(model: &LlamaModel, tokens: &[i32]) -> String {
    tokens
        .iter()
        .map(|&id| {
            String::from_utf8_lossy(
                &model
                    .vocab()
                    .token_to_piece(LlamaToken::new(id), false, None),
            )
            .into_owned()
        })
        .collect()
}

/// Normalized bigram-overlap similarity in [0, 1] (order-sensitive, cheap).
fn similarity(left: &str, right: &str) -> f64 {
    let grams = |text: &str| -> std::collections::HashMap<(char, char), usize> {
        let chars: Vec<char> = text.chars().collect();
        let mut map = std::collections::HashMap::new();
        for pair in chars.windows(2) {
            *map.entry((pair[0], pair[1])).or_insert(0) += 1;
        }
        map
    };
    let (left_grams, right_grams) = (grams(left), grams(right));
    let left_total: usize = left_grams.values().sum();
    let right_total: usize = right_grams.values().sum();
    if left_total == 0 || right_total == 0 {
        return 0.0;
    }
    let overlap: usize = left_grams
        .iter()
        .map(|(gram, count)| (*count).min(*right_grams.get(gram).unwrap_or(&0)))
        .sum();
    2.0 * overlap as f64 / (left_total + right_total) as f64
}

/// First token index where `batched` leaves `baseline`, if any.
fn first_divergence(baseline: &[i32], batched: &[i32]) -> Option<usize> {
    baseline
        .iter()
        .zip(batched)
        .position(|(a, b)| a != b)
        .or_else(|| (baseline.len() != batched.len()).then_some(baseline.len().min(batched.len())))
}

#[test]
#[ignore = "env-gated: run with OCRX_E2E_MODEL=1 (real model + PDFium + golden source)"]
fn p4_1_5_spike_a_batch_divergence_and_spike_b_multi_sequence() {
    if let Some(reason) = skip_reason() {
        eprintln!("skipped: {reason}");
        return;
    }
    let (model_path, mmproj_path) = resolve_model_paths().expect("model assets resolve");
    let pages = render_pages();
    let started = Instant::now();

    let backend = LlamaBackend::init().expect("backend init");
    let model = LlamaModel::load_from_file(
        &backend,
        &model_path,
        &LlamaModelParams::default()
            .with_n_gpu_layers(0)
            .with_split_mode(LlamaSplitMode::None)
            .with_main_gpu(-1),
    )
    .expect("reference GGUF loads");
    let mut mtmd = MtmdContext::init_from_file(
        &mmproj_path.to_string_lossy(),
        &model,
        &MtmdContextParams {
            use_gpu: false,
            image_min_tokens: 1024,
            image_max_tokens: 1024,
            ..Default::default()
        },
    )
    .expect("mmproj loads");
    let make_context = || {
        model
            .new_context(
                &backend,
                LlamaContextParams::default()
                    .with_n_ctx(NonZeroU32::new(20_480))
                    // Spike B finding: the default n_seq_max is 1 and
                    // llama_decode rejects seq_id >= n_seq_max — the windowed
                    // context must declare the slot count (plan P4.3).
                    .with_n_seq_max(4)
                    .with_op_offload(false)
                    .with_offload_kqv(false),
            )
            .expect("context builds")
    };

    // Spike A baseline: every page alone in a fresh context, sequence 0.
    let mut baselines: Vec<Vec<i32>> = Vec::new();
    for (index, (width, height, rgb)) in pages.iter().enumerate() {
        let mut context = make_context();
        let position =
            encode_sequence(&mut mtmd, &mut context, *width, *height, rgb, 0).expect("encode");
        let seed = seed_from_last_eval(&context);
        let mut last = vec![LlamaToken::new(seed)];
        let mut positions = vec![position];
        let streams = decode_shared(&model, &mut context, &mut last, &mut positions);
        let mut full = vec![seed];
        full.extend_from_slice(&streams[0]);
        eprintln!(
            "[spike] baseline page {} = {} tokens",
            index + 1,
            full.len()
        );
        baselines.push(full);
    }

    // Spike A batched runs + Spike B combination behavior.
    let mut report = String::from("{\n  \"token_cap\": 96,\n  \"pages\": 6,\n  \"elapsed_s\": ");
    report.push_str(&format!(
        "{:.1},\n  \"runs\": [",
        started.elapsed().as_secs_f64()
    ));
    let mut feasibility = Vec::new();
    for (run_index, &batch_depth) in BATCH_DEPTHS.iter().enumerate() {
        if run_index > 0 {
            report.push(',');
        }
        report.push_str(&format!(
            "\n    {{\"batch_depth\": {batch_depth}, \"pages\": ["
        ));
        let mut context = make_context();
        for (window_index, window) in pages.chunks(batch_depth).enumerate() {
            if window_index > 0 {
                report.push(',');
            }
            let first_page = window_index * batch_depth;
            let mut positions = Vec::new();
            let mut seeds = Vec::new();
            for (slot, (width, height, rgb)) in window.iter().enumerate() {
                positions.push(
                    encode_sequence(&mut mtmd, &mut context, *width, *height, rgb, slot as i32)
                        .expect("batched encode"),
                );
                // Sample before the next page's eval overwrites the buffer.
                seeds.push(seed_from_last_eval(&context));
            }
            let mut last: Vec<LlamaToken> = seeds.iter().map(|&id| LlamaToken::new(id)).collect();
            let streams = decode_shared(&model, &mut context, &mut last, &mut positions);
            for (slot, stream) in streams.iter().enumerate() {
                let page_index = first_page + slot;
                let mut full = vec![seeds[slot]];
                full.extend_from_slice(stream);
                let baseline = &baselines[page_index];
                let sim = similarity(&to_text(&model, baseline), &to_text(&model, &full));
                feasibility.push(sim);
                report.push_str(&format!(
                    "\n      {{\"page\": {}, \"first_divergence_token\": {}, \"similarity\": {:.4}, \"baseline_tokens\": {}, \"batched_tokens\": {}}}",
                    page_index + 1,
                    match first_divergence(baseline, &full) {
                        Some(index) => format!("{index}"),
                        None => "null".to_owned(),
                    },
                    sim,
                    baseline.len(),
                    full.len(),
                ));
                if slot + 1 < streams.len() {
                    report.push(',');
                }
            }
            // Spike B slot recycling: per-sequence KV removal between windows.
            for slot in 0..window.len() {
                context
                    .kv_cache_seq_rm(slot as i32, Some(0), None)
                    .expect("per-sequence KV recycling");
            }
        }
        report.push_str("]}");
    }
    report.push_str("\n  ]\n}\n");

    let out_path = std::env::var_os("OCRX_SPIKE_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("ocrx-spike-p415.json"));
    std::fs::write(&out_path, &report).expect("spike record writes");
    println!("{report}");
    println!("spike record -> {}", out_path.display());

    // Feasibility assertions (Spike B): every batched run produced output for
    // every page and no similarity collapsed.
    assert_eq!(feasibility.len(), PAGE_COUNT * BATCH_DEPTHS.len());
    for (index, sim) in feasibility.iter().enumerate() {
        assert!(
            *sim > 0.5,
            "run {index}: similarity {sim:.3} collapsed — multi-sequence combination broken"
        );
    }
}
