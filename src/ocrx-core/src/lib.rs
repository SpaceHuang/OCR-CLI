//! Stable domain contracts shared by the OCR pipeline.

pub mod resource_sampler;

use std::io::{self, Read};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

/// Cooperative cancellation token shared by runtime, scan, and CLI layers.
///
/// The first cancellation request marks the current job as cancelled (used by
/// the CLI to stop taking new pages and return exit code 130). A second request
/// escalates to force-exit, mirroring the frozen Ctrl+C contract where the
/// first interrupt is graceful and the second is immediate.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    requests: Arc<AtomicU8>,
}

impl CancellationToken {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request_cancel(&self) {
        self.requests
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                Some(value.saturating_add(1))
            })
            .ok();
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.requests.load(Ordering::SeqCst) >= 1
    }

    #[must_use]
    pub fn force_exit(&self) -> bool {
        self.requests.load(Ordering::SeqCst) >= 2
    }
}

#[allow(clippy::large_stack_arrays)]
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut state = Sha256State::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        state.update(&buffer[..read]);
    }
    Ok(state.finalize())
}

pub struct Sha256State {
    h: [u32; 8],
    length: u64,
    buffer: [u8; 64],
    buffered: usize,
}

impl Sha256State {
    #[must_use]
    #[allow(clippy::new_without_default, clippy::unreadable_literal)]
    pub fn new() -> Self {
        Self {
            h: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            length: 0,
            buffer: [0; 64],
            buffered: 0,
        }
    }
    pub fn update(&mut self, mut data: &[u8]) {
        self.length = self.length.wrapping_add(data.len() as u64);
        if self.buffered != 0 {
            let take = (64 - self.buffered).min(data.len());
            self.buffer[self.buffered..self.buffered + take].copy_from_slice(&data[..take]);
            self.buffered += take;
            data = &data[take..];
            if self.buffered == 64 {
                compress(&mut self.h, &self.buffer);
                self.buffered = 0;
            }
        }
        while data.len() >= 64 {
            compress(&mut self.h, &data[..64]);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.buffer[..data.len()].copy_from_slice(data);
            self.buffered = data.len();
        }
    }
    #[must_use]
    pub fn finalize(mut self) -> String {
        let bit_len = self.length.wrapping_mul(8);
        self.buffer[self.buffered] = 128;
        self.buffered += 1;
        if self.buffered > 56 {
            self.buffer[self.buffered..].fill(0);
            compress(&mut self.h, &self.buffer);
            self.buffered = 0;
        }
        self.buffer[self.buffered..56].fill(0);
        self.buffer[56..].copy_from_slice(&bit_len.to_be_bytes());
        compress(&mut self.h, &self.buffer);
        self.h.iter().map(|v| format!("{v:08x}")).collect()
    }
}

#[allow(clippy::many_single_char_names, clippy::unreadable_literal)]
fn compress(h: &mut [u32; 8], chunk: &[u8]) {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut w = [0u32; 64];
    for i in 0..16 {
        w[i] = u32::from_be_bytes([
            chunk[i * 4],
            chunk[i * 4 + 1],
            chunk[i * 4 + 2],
            chunk[i * 4 + 3],
        ])
    }
    for i in 16..64 {
        let a = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let b = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(a)
            .wrapping_add(w[i - 7])
            .wrapping_add(b)
    }
    let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut z) =
        (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
    for i in 0..64 {
        let s = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let t1 = z
            .wrapping_add(s)
            .wrapping_add((e & f) ^ (!e & g))
            .wrapping_add(K[i])
            .wrapping_add(w[i]);
        let s = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let t2 = s.wrapping_add((a & b) ^ (a & c) ^ (b & c));
        z = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2)
    }
    for (i, v) in [a, b, c, d, e, f, g, z].into_iter().enumerate() {
        h[i] = h[i].wrapping_add(v)
    }
}

#[must_use]
#[allow(clippy::many_single_char_names, clippy::unreadable_literal)]
pub fn sha256(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut m = data.to_vec();
    m.push(128);
    while m.len() % 64 != 56 {
        m.push(0)
    }
    m.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for c in m.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([c[i * 4], c[i * 4 + 1], c[i * 4 + 2], c[i * 4 + 3]])
        }
        for i in 16..64 {
            let a = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let b = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(a)
                .wrapping_add(w[i - 7])
                .wrapping_add(b)
        }
        let (mut a, mut b, mut c0, mut d, mut e, mut f, mut g, mut z) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = z
                .wrapping_add(s)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let t2 = s.wrapping_add((a & b) ^ (a & c0) ^ (b & c0));
            z = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c0;
            c0 = b;
            b = a;
            a = t1.wrapping_add(t2)
        }
        for (i, v) in [a, b, c0, d, e, f, g, z].into_iter().enumerate() {
            h[i] = h[i].wrapping_add(v)
        }
    }
    h.iter().map(|v| format!("{v:08x}")).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobId(pub String);

#[cfg(test)]
mod hash_tests {
    use super::{Sha256State, sha256};
    #[test]
    fn sha256_matches_nist_vectors_and_chunking() {
        assert_eq!(
            sha256(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let mut state = Sha256State::new();
        for chunk in b"abc".chunks(1) {
            state.update(chunk);
        }
        assert_eq!(state.finalize(), sha256(b"abc"));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageDocument {
    pub schema_version: u32,
    pub page_number: u32,
    pub blocks: Vec<PageBlock>,
    pub termination: Termination,
    pub warnings: Vec<PageWarning>,
}

impl PageDocument {
    pub const SCHEMA_VERSION: u32 = 1;

    /// Validates the stable page contract before it can be persisted or rendered.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidPage`] for an unsupported schema or page
    /// number, and [`DomainError::InvalidBbox`] for an invalid visual region.
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.schema_version != Self::SCHEMA_VERSION || self.page_number == 0 {
            return Err(DomainError::InvalidPage);
        }
        if self.blocks.iter().any(|block| {
            matches!(
                block,
                PageBlock::VisualRegion { bbox, .. } if !bbox.is_valid()
            )
        }) {
            return Err(DomainError::InvalidBbox);
        }
        Ok(())
    }

    /// Serializes the validated domain document into the stable JSON shape.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError`] when the document violates the domain invariants.
    pub fn to_json(&self) -> Result<String, DomainError> {
        self.validate()?;
        let blocks = self
            .blocks
            .iter()
            .map(PageBlock::to_json)
            .collect::<Vec<_>>()
            .join(",");
        let warnings = self
            .warnings
            .iter()
            .map(PageWarning::as_str)
            .map(|warning| format!("\"{warning}\""))
            .collect::<Vec<_>>()
            .join(",");
        Ok(format!(
            "{{\"schema_version\":{},\"page_number\":{},\"blocks\":[{}],\"termination\":\"{}\",\"warnings\":[{}]}}",
            self.schema_version,
            self.page_number,
            blocks,
            self.termination.as_str(),
            warnings
        ))
    }

    /// Deserializes the stable JSON shape emitted by [`Self::to_json`].
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidJson`] for malformed or unsupported JSON.
    pub fn from_json(value: &str) -> Result<Self, DomainError> {
        let schema_version = json_number(value, "schema_version")?;
        let page_number = json_number(value, "page_number")?;
        let termination = match json_string(value, "termination")?.as_str() {
            "eos" => Termination::Eos,
            "stop_token" => Termination::StopToken,
            "max_tokens" => Termination::MaxTokens,
            "invalid" => Termination::Invalid,
            _ => return Err(DomainError::InvalidJson),
        };
        let warnings_value = value
            .split_once("\"warnings\":[")
            .and_then(|(_, rest)| rest.split_once(']'))
            .ok_or(DomainError::InvalidJson)?
            .0;
        let warnings = if warnings_value.trim().is_empty() {
            Vec::new()
        } else {
            warnings_value
                .split(',')
                .map(|item| match item.trim().trim_matches('"') {
                    "truncated" => Ok(PageWarning::Truncated),
                    "repetition_removed" => Ok(PageWarning::RepetitionRemoved),
                    "render_scaled" => Ok(PageWarning::RenderScaled),
                    _ => Err(DomainError::InvalidJson),
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        let blocks_value = value
            .split_once("\"blocks\":[")
            .and_then(|(_, rest)| rest.split_once("],\"termination\""))
            .ok_or(DomainError::InvalidJson)?
            .0;
        let blocks = if blocks_value.trim().is_empty() {
            Vec::new()
        } else {
            blocks_value
                .split("},{")
                .map(parse_block)
                .collect::<Result<Vec<_>, _>>()?
        };
        let document = Self {
            schema_version,
            page_number,
            blocks,
            termination,
            warnings,
        };
        document.validate()?;
        Ok(document)
    }
}

fn json_number(value: &str, key: &str) -> Result<u32, DomainError> {
    value
        .split_once(&format!("\"{key}\":"))
        .and_then(|(_, rest)| rest.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|number| number.parse().ok())
        .ok_or(DomainError::InvalidJson)
}

fn json_string(value: &str, key: &str) -> Result<String, DomainError> {
    value
        .split_once(&format!("\"{key}\":\""))
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(string, _)| {
            string
                .replace("\\n", "\n")
                .replace("\\\"", "\"")
                .replace("\\\\", "\\")
        })
        .ok_or(DomainError::InvalidJson)
}

fn parse_block(value: &str) -> Result<PageBlock, DomainError> {
    let value = value.trim_matches('{').trim_matches('}');
    let kind = json_string(value, "type")?;
    match kind.as_str() {
        "markdown" => Ok(PageBlock::Markdown {
            content: json_string(value, "content")?,
        }),
        "formula" => Ok(PageBlock::Formula {
            latex: json_string(value, "latex")?,
            display: value.contains("\"display\":true"),
        }),
        "html_table" => Ok(PageBlock::HtmlTable {
            html: json_string(value, "html")?,
        }),
        "visual_region" => {
            let bbox = value
                .split_once("\"bbox\":{")
                .and_then(|(_, rest)| rest.split_once('}'))
                .ok_or(DomainError::InvalidJson)?
                .0;
            let number =
                |key| u16::try_from(json_number(bbox, key)?).map_err(|_| DomainError::InvalidJson);
            Ok(PageBlock::VisualRegion {
                bbox: NormalizedBbox::new(
                    number("left")?,
                    number("top")?,
                    number("right")?,
                    number("bottom")?,
                )?,
                source_order: json_number(value, "source_order")?,
            })
        }
        _ => Err(DomainError::InvalidJson),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageBlock {
    Markdown {
        content: String,
    },
    Formula {
        latex: String,
        display: bool,
    },
    HtmlTable {
        html: String,
    },
    VisualRegion {
        bbox: NormalizedBbox,
        source_order: u32,
    },
}

impl PageBlock {
    fn to_json(&self) -> String {
        match self {
            Self::Markdown { content } => format!(
                "{{\"type\":\"markdown\",\"content\":\"{}\"}}",
                json_escape(content)
            ),
            Self::Formula { latex, display } => format!(
                "{{\"type\":\"formula\",\"latex\":\"{}\",\"display\":{display}}}",
                json_escape(latex)
            ),
            Self::HtmlTable { html } => format!(
                "{{\"type\":\"html_table\",\"html\":\"{}\"}}",
                json_escape(html)
            ),
            Self::VisualRegion { bbox, source_order } => format!(
                "{{\"type\":\"visual_region\",\"bbox\":{{\"left\":{},\"top\":{},\"right\":{},\"bottom\":{}}},\"source_order\":{source_order}}}",
                bbox.left, bbox.top, bbox.right, bbox.bottom
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NormalizedBbox {
    pub left: u16,
    pub top: u16,
    pub right: u16,
    pub bottom: u16,
}

impl NormalizedBbox {
    /// Creates a normalized bounding box after validating its half-open bounds.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidBbox`] when the box has zero area or uses
    /// coordinates outside the `[0, 1000)` coordinate space.
    pub fn new(left: u16, top: u16, right: u16, bottom: u16) -> Result<Self, DomainError> {
        let bbox = Self {
            left,
            top,
            right,
            bottom,
        };
        bbox.is_valid()
            .then_some(bbox)
            .ok_or(DomainError::InvalidBbox)
    }

    #[must_use]
    pub fn is_valid(self) -> bool {
        self.left < self.right && self.top < self.bottom && self.right < 1000 && self.bottom < 1000
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Termination {
    Eos,
    StopToken,
    MaxTokens,
    Invalid,
}

impl Termination {
    fn as_str(self) -> &'static str {
        match self {
            Self::Eos => "eos",
            Self::StopToken => "stop_token",
            Self::MaxTokens => "max_tokens",
            Self::Invalid => "invalid",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageWarning {
    Truncated,
    RepetitionRemoved,
    RenderScaled,
}

impl PageWarning {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Truncated => "truncated",
            Self::RepetitionRemoved => "repetition_removed",
            Self::RenderScaled => "render_scaled",
        }
    }
}

fn json_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelId {
    OvisOcr2,
}

impl ModelId {
    pub const AS_STR: &str = "ovisocr2";

    /// Parses the only model supported by the v0.1 contract.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::UnsupportedModel`] for every other identifier.
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        (value == Self::AS_STR)
            .then_some(Self::OvisOcr2)
            .ok_or(DomainError::UnsupportedModel)
    }
}

/// Frozen runtime binding compatibility for the community OvisOCR2 asset.
///
/// Single source of truth shared across macOS and Windows: the `llama-cpp-2`
/// wrapper version, the `llama-cpp-sys-2` sys crate version, the embedded
/// `llama.cpp` upstream commit, and the adapter id. The per-target
/// `Cargo.toml` dependencies in `ocrx-runtime` and
/// `ocrx_models::validate_runtime_adapter_compatibility` must agree with these
/// values; platform differences live only in build features and probing, never
/// in scattered version constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeCompatibility {
    /// `llama-cpp-2` wrapper crate version.
    pub llama_cpp_2_version: &'static str,
    /// `llama-cpp-sys-2` sys crate version.
    pub llama_cpp_sys_2_version: &'static str,
    /// Embedded `llama.cpp` upstream commit (short SHA), recorded by the
    /// WP.2 build freeze (`docs/develop/runtime-windows-alignment-*.md`).
    pub llama_cpp_commit: &'static str,
    /// Adapter id this runtime binding serves (`ovisocr2`).
    pub adapter_id: &'static str,
}

/// The pinned runtime binding shared by macOS and Windows builds.
pub const RUNTIME_COMPATIBILITY: RuntimeCompatibility = RuntimeCompatibility {
    llama_cpp_2_version: "0.1.152",
    llama_cpp_sys_2_version: "0.1.153",
    // llama.cpp commit embedded in llama-cpp-sys-2 0.1.153, verified by
    // file-hash match against upstream; frozen by the WP.2 Windows build
    // (docs/develop/runtime-windows-alignment-2026-08-14.md).
    llama_cpp_commit: "9e3b928",
    adapter_id: ModelId::AS_STR,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Auto,
    Compact,
    Balanced,
    Quality,
    Reference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    Auto,
    Cpu,
    Metal,
    Cuda,
    Vulkan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    UnsupportedModel,
    InvalidBbox,
    InvalidPage,
    InvalidJson,
    InvalidError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Success,
    Usage,
    Input,
    Model,
    Download,
    Runtime,
    Parse,
    Integrity,
    Output,
    Partial,
    Cancelled,
}

impl ErrorKind {
    #[must_use]
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::Usage => 2,
            Self::Input => 3,
            Self::Model => 4,
            Self::Download => 5,
            Self::Runtime => 6,
            Self::Parse => 7,
            Self::Integrity => 8,
            Self::Output => 9,
            Self::Partial => 10,
            Self::Cancelled => 130,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppError {
    pub kind: ErrorKind,
    pub code: u8,
    pub message: String,
    pub cause: Option<String>,
    pub suggestion: Option<String>,
}

impl AppError {
    pub fn new(kind: ErrorKind, code: u8, message: impl Into<String>) -> Self {
        Self {
            kind,
            code,
            message: message.into(),
            cause: None,
            suggestion: None,
        }
    }

    #[must_use]
    pub fn from_kind(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self::new(kind, kind.exit_code(), message)
    }

    #[must_use]
    pub fn with_context(mut self, cause: impl Into<String>, suggestion: impl Into<String>) -> Self {
        self.cause = Some(cause.into());
        self.suggestion = Some(suggestion.into());
        self
    }

    /// Checks that the exit code matches the stable mapping for [`Self::kind`].
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidError`] when the code and kind disagree.
    pub fn validate(&self) -> Result<(), DomainError> {
        (self.code == self.kind.exit_code())
            .then_some(())
            .ok_or(DomainError::InvalidError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_compatibility_records_frozen_llama_cpp_commit() {
        // WP.2 build freeze: the shared binding must name the llama.cpp commit
        // embedded in llama-cpp-sys-2 0.1.153, so W0.1's structured comparison
        // and every compatibility report reference one pinned runtime.
        let compat = RUNTIME_COMPATIBILITY;
        assert!(
            !compat.llama_cpp_commit.is_empty(),
            "llama_cpp_commit is filled by the WP.2 Windows build freeze"
        );
        assert!(
            compat
                .llama_cpp_commit
                .chars()
                .all(|c| c.is_ascii_hexdigit()),
            "llama_cpp_commit is a hex SHA fragment, not free text"
        );
        assert_eq!(compat.llama_cpp_2_version, "0.1.152");
        assert_eq!(compat.llama_cpp_sys_2_version, "0.1.153");
        assert_eq!(compat.adapter_id, "ovisocr2");
    }

    #[test]
    fn only_ovisocr2_is_a_valid_model() {
        assert_eq!(ModelId::parse("ovisocr2"), Ok(ModelId::OvisOcr2));
        assert_eq!(ModelId::parse("other"), Err(DomainError::UnsupportedModel));
    }

    #[test]
    fn bbox_uses_half_open_1000_space() {
        assert!(
            NormalizedBbox {
                left: 0,
                top: 0,
                right: 999,
                bottom: 999
            }
            .is_valid()
        );
        assert!(
            !NormalizedBbox {
                left: 0,
                top: 0,
                right: 1000,
                bottom: 999
            }
            .is_valid()
        );
        assert!(
            !NormalizedBbox {
                left: 10,
                top: 0,
                right: 10,
                bottom: 2
            }
            .is_valid()
        );
    }

    #[test]
    fn bbox_constructor_rejects_invalid_values() {
        assert_eq!(
            NormalizedBbox::new(0, 0, 999, 999),
            Ok(NormalizedBbox {
                left: 0,
                top: 0,
                right: 999,
                bottom: 999
            })
        );
        assert_eq!(
            NormalizedBbox::new(4, 2, 4, 8),
            Err(DomainError::InvalidBbox)
        );
    }

    #[test]
    fn app_error_keeps_three_part_user_context() {
        let error = AppError::new(ErrorKind::Input, 2, "invalid page").with_context(
            "page 0 is outside the document",
            "use a page number from 1 onward",
        );
        assert_eq!(error.code, 2);
        assert!(error.cause.is_some());
        assert!(error.suggestion.is_some());
    }

    #[test]
    fn error_kinds_have_stable_exit_codes() {
        let cases = [
            (ErrorKind::Success, 0),
            (ErrorKind::Usage, 2),
            (ErrorKind::Input, 3),
            (ErrorKind::Model, 4),
            (ErrorKind::Download, 5),
            (ErrorKind::Runtime, 6),
            (ErrorKind::Parse, 7),
            (ErrorKind::Integrity, 8),
            (ErrorKind::Output, 9),
            (ErrorKind::Partial, 10),
            (ErrorKind::Cancelled, 130),
        ];
        for (kind, expected) in cases {
            assert_eq!(kind.exit_code(), expected);
            assert_eq!(AppError::from_kind(kind, "failure").code, expected);
        }
        assert!(
            AppError::from_kind(ErrorKind::Parse, "failure")
                .validate()
                .is_ok()
        );
        assert_eq!(
            AppError::new(ErrorKind::Parse, 6, "failure").validate(),
            Err(DomainError::InvalidError)
        );
    }

    #[test]
    fn page_document_validation_rejects_unknown_schema_and_zero_page() {
        let document = PageDocument {
            schema_version: PageDocument::SCHEMA_VERSION + 1,
            page_number: 1,
            blocks: vec![],
            termination: Termination::Eos,
            warnings: vec![],
        };
        assert_eq!(document.validate(), Err(DomainError::InvalidPage));

        let document = PageDocument {
            schema_version: PageDocument::SCHEMA_VERSION,
            page_number: 0,
            blocks: vec![],
            termination: Termination::Eos,
            warnings: vec![],
        };
        assert_eq!(document.validate(), Err(DomainError::InvalidPage));
    }

    #[test]
    fn page_document_json_has_stable_schema_golden() {
        let document = PageDocument {
            schema_version: 1,
            page_number: 2,
            blocks: vec![
                PageBlock::Markdown {
                    content: "Hello\nworld".to_owned(),
                },
                PageBlock::Formula {
                    latex: "x^2".to_owned(),
                    display: true,
                },
                PageBlock::HtmlTable {
                    html: "<table><tr><td>A</td></tr></table>".to_owned(),
                },
                PageBlock::VisualRegion {
                    bbox: NormalizedBbox::new(1, 2, 10, 20).unwrap(),
                    source_order: 3,
                },
            ],
            termination: Termination::StopToken,
            warnings: vec![PageWarning::RenderScaled],
        };
        assert_eq!(
            document.to_json().unwrap(),
            "{\"schema_version\":1,\"page_number\":2,\"blocks\":[{\"type\":\"markdown\",\"content\":\"Hello\\nworld\"},{\"type\":\"formula\",\"latex\":\"x^2\",\"display\":true},{\"type\":\"html_table\",\"html\":\"<table><tr><td>A</td></tr></table>\"},{\"type\":\"visual_region\",\"bbox\":{\"left\":1,\"top\":2,\"right\":10,\"bottom\":20},\"source_order\":3}],\"termination\":\"stop_token\",\"warnings\":[\"render_scaled\"]}"
        );
        let encoded = document.to_json().unwrap();
        assert_eq!(PageDocument::from_json(&encoded).unwrap(), document);
    }

    #[test]
    fn page_document_empty_and_warning_variants_round_trip() {
        for (termination, warnings) in [
            (Termination::Eos, vec![]),
            (Termination::MaxTokens, vec![PageWarning::Truncated]),
            (
                Termination::Invalid,
                vec![PageWarning::RepetitionRemoved, PageWarning::RenderScaled],
            ),
        ] {
            let document = PageDocument {
                schema_version: 1,
                page_number: 1,
                blocks: vec![],
                termination,
                warnings,
            };
            let encoded = document.to_json().unwrap();
            assert_eq!(PageDocument::from_json(&encoded).unwrap(), document);
        }
    }

    #[test]
    fn page_document_validation_rejects_invalid_visual_region() {
        let document = PageDocument {
            schema_version: PageDocument::SCHEMA_VERSION,
            page_number: 1,
            blocks: vec![PageBlock::VisualRegion {
                bbox: NormalizedBbox {
                    left: 10,
                    top: 10,
                    right: 10,
                    bottom: 20,
                },
                source_order: 0,
            }],
            termination: Termination::Eos,
            warnings: vec![],
        };
        assert_eq!(document.validate(), Err(DomainError::InvalidBbox));
    }
}
