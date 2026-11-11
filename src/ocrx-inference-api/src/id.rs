//! Identifier value types for the inference platform.
//!
//! Model IDs are no longer a single-variant enum: they are catalog-resolved
//! value types. The built-in catalog still only registers `ovisocr2`, so the
//! external behavior stays single-model (plan §5.1).

use serde::{Deserialize, Serialize};

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Builds a validated identifier.
            ///
            /// # Errors
            ///
            /// Returns [`InferenceError::InvalidTask`] when the value is empty,
            /// whitespace-padded, longer than 128 chars, or carries control
            /// characters.
            pub fn new(value: impl Into<String>) -> Result<Self, crate::InferenceError> {
                let value = value.into();
                if value.is_empty()
                    || value.trim() != value
                    || value.len() > 128
                    || value.chars().any(char::is_control)
                {
                    return Err(crate::InferenceError::InvalidTask(format!(
                        "invalid identifier: {value:?}"
                    )));
                }
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                self.0.as_str()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

id_type!(
    /// Resolved model identifier (catalog value type; currently `ovisocr2`).
    ModelId
);
id_type!(
    /// Resolved model profile identifier (`compact`/`balanced`/`quality`/`reference`).
    ProfileId
);
id_type!(
    /// Protocol adapter identifier (currently `ovisocr2`).
    AdapterId
);
id_type!(
    /// Internal runtime identifier (currently fixed `llama.cpp`).
    RuntimeId
);
id_type!(
    /// Actually executed backend identifier (lowercase: `cpu`/`metal`/`cuda`/`vulkan`).
    BackendId
);
id_type!(
    /// Named asset role inside a manifest (currently `model` and `mmproj`).
    AssetRole
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InferenceError;

    #[test]
    fn identifiers_accept_clean_values_and_reject_bad_ones() {
        assert!(ModelId::new("ovisocr2").is_ok());
        assert!(ModelId::new(" ovisocr2").is_err());
        assert!(ModelId::new(String::new()).is_err());
        assert!(ModelId::new("bad\nid").is_err());
        assert!(ModelId::new("x".repeat(129)).is_err());
    }

    #[test]
    fn identifiers_serialize_as_transparent_strings() {
        assert_eq!(
            serde_json::to_string(&ModelId::new("ovisocr2").unwrap()).unwrap(),
            "\"ovisocr2\""
        );
        assert_eq!(
            serde_json::from_str::<ModelId>("\"ovisocr2\"").unwrap(),
            ModelId::new("ovisocr2").unwrap()
        );
    }

    #[test]
    fn identifiers_sort_and_display_stably() {
        let mut profiles = [
            ProfileId::new("quality").unwrap(),
            ProfileId::new("compact").unwrap(),
        ];
        profiles.sort();
        assert_eq!(
            profiles.iter().map(ProfileId::as_str).collect::<Vec<_>>(),
            ["compact", "quality"]
        );
        assert_eq!(
            BackendId::new("cpu").unwrap().to_string(),
            "cpu"
        );
    }

    #[test]
    fn invalid_identifier_reports_invalid_task_category() {
        assert!(matches!(
            ModelId::new(""),
            Err(InferenceError::InvalidTask(_))
        ));
    }
}
