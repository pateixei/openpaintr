use std::{fmt, io};

/// Carries a stable localization key while preserving the core error's diagnostic text.
pub struct Diagnostic {
    key: &'static str,
    message: &'static str,
}

/// A stable localization key with its English fallback text, which matches
/// `translations/en-us.json` (checked by a test).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Message {
    pub key: &'static str,
    pub text: &'static str,
}

macro_rules! messages {
    ($($name:ident = $key:literal => $text:literal;)*) => {
        $(pub(crate) const $name: super::Message = super::Message { key: $key, text: $text };)*

        #[cfg(test)]
        pub(crate) const ALL: &[super::Message] = &[$($name),*];
    };
}

/// Core error messages, one entry per localization key.
pub(crate) mod msg {
    messages! {
        ANIMATED_PNG = "error.animated-png" => "Animated PNG is not supported yet.";
        EMPTY_CANVAS = "error.empty-canvas" => "The canvas is empty.";
        INCOMPATIBLE_LAYER_DATA = "error.incompatible-layer-data" => "Incompatible layer data.";
        INVALID_BLEND_MODE = "error.invalid-blend-mode" => "Invalid blend mode.";
        INVALID_COMPOSITE_REGION = "error.invalid-composite-region" => "Invalid compositing region.";
        INVALID_LAYER_INDEX = "error.invalid-layer-index" => "Invalid layer index.";
        INVALID_LAYER_NAME = "error.invalid-layer-name" => "Invalid layer name.";
        INVALID_LAYER_POSITION = "error.invalid-layer-position" => "Invalid layer position.";
        INVALID_LAYER_SIZE = "error.invalid-layer-size" => "Invalid layer size.";
        INVALID_PNG_SIZE = "error.invalid-png-size" => "Invalid PNG dimensions.";
        LAST_LAYER = "error.last-layer" => "The document must have at least one layer.";
        LAYER_MISSING = "error.layer-missing" => "Layer not found.";
        NO_LAYER_BELOW = "error.no-layer-below" => "There is no layer below to merge with.";
        PDN_DECODE_LIMIT = "error.pdn-decode-limit" => "PDN decoding limit exceeded.";
        PDN_EMPTY = "error.pdn-empty" => "The PDN contains no image or layers.";
        PDN_ENCODE = "error.pdn-encode" => "Could not encode the PDN document.";
        PDN_FILE_SIZE = "error.pdn-file-size" => "PDN file exceeds 256 MiB.";
        PDN_INVALID = "error.pdn-invalid" => "The PDN file is invalid, incomplete, or incompatible.";
        PDN_LAYER_LIMIT = "error.pdn-layer-limit" => "The PDN layers exceed the pixel limit.";
        PDN_LAYER_NAME_MODE = "error.pdn-layer-name-mode" => "A layer name or blend mode is incompatible with PDN.";
        PDN_LAYER_SIZE = "error.pdn-layer-size" => "Invalid PDN layer dimensions.";
        PDN_OUT_OF_MEMORY = "error.pdn-out-of-memory" => "Not enough memory to open the PDN.";
        PDN_SAVE_MEMORY = "error.pdn-save-memory" => "Not enough memory to save the PDN.";
        PDN_VARIANT = "error.pdn-variant" => "Unsupported PDN variant.";
        PIXEL_OUTSIDE = "error.pixel-outside" => "The pixel is outside the canvas.";
        REGION_TOO_LARGE = "error.region-too-large" => "The region is too large.";
        TOTAL_LAYER_LIMIT = "error.total-layer-limit" => "The layers exceed the total pixel limit.";
    }
}

impl Diagnostic {
    pub(crate) fn io(kind: io::ErrorKind, message: Message) -> io::Error {
        io::Error::new(
            kind,
            Self {
                key: message.key,
                message: message.text,
            },
        )
    }

    pub fn key(&self) -> &'static str {
        self.key
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.write_str(self.message)
    }
}

impl fmt::Debug for Diagnostic {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.message, fmt)
    }
}

impl std::error::Error for Diagnostic {}

#[cfg(all(test, feature = "gui"))]
mod tests {
    #[test]
    fn fallback_messages_match_the_english_catalog() {
        let catalog: std::collections::BTreeMap<String, String> =
            serde_json::from_str(include_str!("../translations/en-us.json")).expect("valid catalog");
        for message in super::msg::ALL {
            assert_eq!(catalog.get(message.key).map(String::as_str), Some(message.text), "{}", message.key);
        }
    }
}
