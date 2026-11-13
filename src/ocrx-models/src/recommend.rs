//! Model profile recommendation (plan §5.6): pure data rule over a memory
//! input. Hardware probing does NOT live here — the device service owns it.

use ocrx_inference_api::{InferenceError, ProfileId};

/// Frozen memory recommendation ladder (plan §2.3, identical to the legacy
/// rule): below 6 GiB nothing passes; below 12 → compact; below 24 →
/// balanced; otherwise quality. A blocked first choice falls back to
/// compact when available.
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
    if preferred != "compact" && available_profiles.contains(&"compact") {
        return Ok(ProfileId::new("compact").expect("ladder profile is valid"));
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
    fn blocked_first_choice_falls_back_to_compact_only() {
        assert_eq!(
            recommend_profile(&["compact"], 24).unwrap().as_str(),
            "compact"
        );
        assert!(matches!(
            recommend_profile(&["balanced"], 8),
            Err(InferenceError::UnsupportedProfile(_))
        ));
        assert!(matches!(
            recommend_profile(&[], 8),
            Err(InferenceError::UnsupportedProfile(_))
        ));
    }
}
