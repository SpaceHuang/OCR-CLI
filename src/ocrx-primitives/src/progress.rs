//! Progress events emitted by a generating backend.
//!
//! `elapsed_seconds` is the monotonic time since generation started. A
//! backend must call the progress observer at least once per elapsed second
//! (and may call it more often); the CLI maps those calls to the visible
//! heartbeat/page_progress contract.
//!
//! One documented exception: the image/prompt encoding phase (mmproj eval) is
//! a single blocking native call that cannot tick — the backend emits one
//! event when encoding starts and the per-second cadence applies to the
//! decode loop that follows.

/// Progress emitted by a backend while it generates one task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GenerationProgress {
    pub generated_tokens: u32,
    pub elapsed_seconds: u64,
}

#[cfg(test)]
mod tests {
    use super::GenerationProgress;

    #[test]
    fn progress_is_copyable_and_comparable() {
        let event = GenerationProgress {
            generated_tokens: 12,
            elapsed_seconds: 3,
        };
        assert_eq!(
            event,
            GenerationProgress {
                generated_tokens: 12,
                elapsed_seconds: 3
            }
        );
        let copied = event;
        assert_eq!(copied.generated_tokens, 12);
    }
}
