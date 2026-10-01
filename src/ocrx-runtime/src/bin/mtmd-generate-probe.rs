//! Standalone MTMD probe binary.
//!
//! `llama_cpp_2` (with `mtmd`) is a declared dependency on macOS and, since
//! the WP.2 Windows build freeze, on Windows too — matching the
//! `llama_mtmd` runtime gate. The probe builds on both; other targets get a
//! diagnostic so the workspace still compiles everywhere.

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod probe {
    use std::num::NonZeroU32;
    use std::path::Path;

    use llama_cpp_2::context::params::LlamaContextParams;
    use llama_cpp_2::llama_backend::LlamaBackend;
    use llama_cpp_2::llama_batch::LlamaBatch;
    use llama_cpp_2::model::{
        LlamaModel,
        params::{LlamaModelParams, LlamaSplitMode},
    };
    use llama_cpp_2::mtmd::{
        MtmdBitmap, MtmdContext, MtmdContextParams, MtmdInputText, mtmd_default_marker,
    };
    use llama_cpp_2::token::LlamaToken;

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let model_path = std::env::var("OCRX_RUNTIME_MODEL")?;
        let mmproj_path = std::env::var("OCRX_RUNTIME_MMPROJ")?;
        let image_path = std::env::var("OCRX_RUNTIME_IMAGE")?;
        let backend = LlamaBackend::init()?;
        let use_gpu = std::env::var("OCRX_RUNTIME_USE_GPU").map_or(true, |v| v != "0");
        let model_layers = if use_gpu { u32::MAX } else { 0 };
        let model = LlamaModel::load_from_file(
            &backend,
            Path::new(&model_path),
            &LlamaModelParams::default()
                .with_n_gpu_layers(model_layers)
                .with_split_mode(LlamaSplitMode::None),
        )?;
        let mtmd = MtmdContext::init_from_file(
            &mmproj_path,
            &model,
            &MtmdContextParams {
                use_gpu,
                image_min_tokens: 1024,
                image_max_tokens: 1024,
                ..Default::default()
            },
        )?;
        let bitmap = MtmdBitmap::from_file(&mtmd, &image_path, false)?;
        let prompt = std::env::var("OCRX_RUNTIME_PROMPT").unwrap_or_else(|_| "\nExtract all readable content from the image in natural human reading order and output the result as a single Markdown document. For charts or images, represent them using an HTML image tag: <img src=\"images/bbox_{left}_{top}_{right}_{bottom}.jpg\" />, where left, top, right, bottom are bounding box coordinates scaled to [0, 1000). Format formulas as LaTeX. Format tables as HTML: <table>...</table>. Transcribe all other text as standard Markdown.\nPreserve the original text without translation or paraphrasing.".to_owned());
        let chunks = mtmd.tokenize(
            MtmdInputText {
                text: format!("{}{}", mtmd_default_marker(), prompt),
                add_special: true,
                parse_special: true,
            },
            &[&bitmap],
        )?;
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(16384))
            .with_op_offload(use_gpu)
            .with_offload_kqv(use_gpu);
        let mut ctx = model.new_context(&backend, ctx_params)?;
        let n_past = chunks.eval_chunks(&mtmd, &ctx, 0, 0, 512, true)?;
        let mut output = String::new();
        let mut termination = "max_tokens";
        let max_tokens = std::env::var("OCRX_RUNTIME_MAX_TOKENS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(16_384usize);
        let cancel_after = std::env::var("OCRX_RUNTIME_CANCEL_AFTER")
            .ok()
            .and_then(|v| v.parse().ok());
        for (generated, _) in (0..max_tokens).enumerate() {
            if cancel_after.is_some_and(|limit| generated >= limit) {
                termination = "cancelled";
                break;
            }
            let logits = ctx.get_logits();
            let (index, _) = logits
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.total_cmp(b))
                .ok_or("empty logits")?;
            let token = LlamaToken::new(i32::try_from(index)?);
            if token == model.token_eos() {
                termination = "eos";
                break;
            }
            output.push_str(&String::from_utf8_lossy(
                &model.token_to_piece_bytes(token, 64, false, None)?,
            ));
            if generated % 16 == 0 {
                eprintln!("runtime_progress generated_tokens={}", generated + 1);
            }
            let mut batch = LlamaBatch::new(1, 1);
            let token_position = n_past + i32::try_from(generated)?;
            batch.add(token, token_position, &[0], true)?;
            ctx.decode(&mut batch)?;
        }
        eprintln!("runtime_termination={termination}");
        println!("{output}");
        Ok(())
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    probe::run()
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn main() {
    eprintln!(
        "mtmd-generate-probe: the llama_cpp_2 runtime is not built on this target; \
         run on macOS or Windows (plan WP.2/W3.1)."
    );
    std::process::exit(1);
}
