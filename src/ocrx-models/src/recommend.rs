//! Model profile recommendation (plan §5.6): pure data rule over a memory
//! input. Hardware probing does NOT live here — the device service owns it.

use ocrx_inference_api::{InferenceError, ProfileId};

/// Frozen memory recommendation ladder (plan §2.3, identical to the legacy
/// rule): below 6 GiB nothing passes; below 12 → compact; below 24 →
/// balanced; otherwise quality. A blocked first choice falls back to the
/// model's declaration-order first available profile (D-7, v1.4 review
/// BLOCK-1): over a full four-rung ladder that is `compact`, so OvisOCR2
/// production behavior is byte-identical to the historical compact fallback,
/// and every non-empty profile subset gets a deterministic auto answer.
///
/// The caller passes profiles in declaration order (ascending size; the
/// catalog's frozen per-model order).
///
/// # Panics
///
/// Never: the ladder profile names are compile-time constants validated by
/// tests.
pub fn recommend_profile(
    available_profiles: &[&str],
    memory_gb: u64,
) -> Result<ProfileId, InferenceError> {
    let memory_floor = || {
        InferenceError::UnsupportedProfile(format!(
            "memory below the 6 GiB profile floor ({memory_gb} GiB)"
        ))
    };
    let preferred = if memory_gb < 6 {
        return Err(memory_floor());
    } else if memory_gb < 12 {
        "compact"
    } else if memory_gb < 24 {
        "balanced"
    } else {
        "quality"
    };
    if available_profiles.contains(&preferred) {
        return Ok(ProfileId::new(preferred).expect("ladder profile is valid"));
    }
    if let Some(first_available) = available_profiles.first() {
        return Ok(ProfileId::new(*first_available).expect("declared profile is valid"));
    }
    Err(InferenceError::UnsupportedProfile(format!(
        "no profile passes for {memory_gb} GiB with the available profile set"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommendation_matches_the_frozen_memory_ladder() {
        let all = ["compact", "balanced", "quality", "reference"];
        match recommend_profile(&all, 4) {
            Err(InferenceError::UnsupportedProfile(detail)) => {
                assert!(detail.contains("6 GiB"), "detail: {detail}");
            }
            other => panic!("expected unsupported-profile, got {other:?}"),
        }
        assert_eq!(recommend_profile(&all, 8).unwrap().as_str(), "compact");
        assert_eq!(recommend_profile(&all, 16).unwrap().as_str(), "balanced");
        assert_eq!(recommend_profile(&all, 24).unwrap().as_str(), "quality");
        assert_eq!(recommend_profile(&all, 128).unwrap().as_str(), "quality");
    }

    #[test]
    fn blocked_first_choice_falls_back_to_the_first_available_profile() {
        // D-7 (variable profile set): the fallback is the model's
        // declaration-order first available profile, not a fixed `compact`.
        // Over the full OvisOCR2 ladder the first declared profile IS
        // compact, so production behavior is byte-identical to the
        // historical compact fallback.
        assert_eq!(
            recommend_profile(&["compact"], 24).unwrap().as_str(),
            "compact"
        );
        assert_eq!(
            recommend_profile(&["compact", "balanced"], 64)
                .unwrap()
                .as_str(),
            "compact"
        );
        // C15 subsets (declaration order = ascending size): every non-empty
        // subset has a deterministic auto answer.
        assert_eq!(
            recommend_profile(&["balanced"], 8).unwrap().as_str(),
            "balanced"
        );
        assert_eq!(
            recommend_profile(&["quality"], 8).unwrap().as_str(),
            "quality"
        );
        assert_eq!(
            recommend_profile(&["quality"], 128).unwrap().as_str(),
            "quality"
        );
        assert_eq!(
            recommend_profile(&["balanced", "quality"], 8)
                .unwrap()
                .as_str(),
            "balanced"
        );
        assert_eq!(
            recommend_profile(&["reference"], 8).unwrap().as_str(),
            "reference"
        );
        // The empty set still fails loudly.
        assert!(matches!(
            recommend_profile(&[], 8),
            Err(InferenceError::UnsupportedProfile(_))
        ));
        // The 6 GiB resource floor applies to every model (frozen semantics).
        match recommend_profile(&["quality"], 4) {
            Err(InferenceError::UnsupportedProfile(detail)) => {
                assert!(detail.contains("6 GiB"), "detail: {detail}");
            }
            other => panic!("expected unsupported-profile, got {other:?}"),
        }
    }
}
