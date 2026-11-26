use std::{fmt, io};

/// Carries a stable localization key while preserving the core error's diagnostic text.
pub struct Diagnostic {
    key: &'static str,
    message: &'static str,
}

impl Diagnostic {
    pub(crate) fn io(kind: io::ErrorKind, key: &'static str, message: &'static str) -> io::Error {
        io::Error::new(kind, Self { key, message })
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
