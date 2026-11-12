//! OCR business parsing pipeline (plan §4.1): prompt construction, Ovis
//! text parsing, markdown link handling, and page artifact assembly.

pub mod artifacts;
pub mod markdown;
pub mod parser;
pub mod prompt;

/// Frozen output token cap (D5.2): the manifest safety limit.
pub const MAX_OUTPUT_TOKENS: u32 = 16_384;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OvisRequest {
    pub temperature_milli: u32,
    pub max_output_tokens: u32,
    pub thinking: bool,
}

#[must_use]
pub fn request(max_output_tokens: u32) -> OvisRequest {
    OvisRequest {
        temperature_milli: 0,
        max_output_tokens,
        thinking: false,
    }
}

/// Builds a request only when the requested token budget is within the locked manifest limit.
pub fn try_request(max_output_tokens: u32) -> Result<OvisRequest, &'static str> {
    if max_output_tokens == 0 || max_output_tokens > MAX_OUTPUT_TOKENS {
        return Err("max-output-tokens exceeds the manifest safety limit");
    }
    Ok(request(max_output_tokens))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_disables_thinking_and_uses_zero_temperature() {
        assert_eq!(
            request(512),
            OvisRequest {
                temperature_milli: 0,
                max_output_tokens: 512,
                thinking: false
            }
        );
    }

    #[test]
    fn request_rejects_tokens_above_manifest_limit() {
        assert!(try_request(MAX_OUTPUT_TOKENS).is_ok());
        assert!(try_request(MAX_OUTPUT_TOKENS + 1).is_err());
    }
}
