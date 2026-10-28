//! Typed refusals: every domain failure in torori is one of these.
//!
//! Handlers return `Err(TororiError)` and the server layer renders
//! it as an `isError` tool result whose text payload is
//! `{"error":{"code": <stable code>, "message": <detail>}}` — the
//! fleet convention (WS1). Protocol faults (unknown tool, malformed
//! framing) never travel this path; they are protocol errors owned
//! by the era guard / SDK envelope.

use std::fmt;

/// A domain refusal with a stable machine-readable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TororiError {
    pub code: &'static str,
    pub message: String,
}

impl TororiError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        TororiError {
            code,
            message: message.into(),
        }
    }

    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::new("invalid_input", message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("not_found", message)
    }

    /// Render as the JSON text payload of an `isError` result.
    pub fn to_payload(&self) -> String {
        serde_json::json!({"error": {"code": self.code, "message": self.message}}).to_string()
    }
}

impl fmt::Display for TororiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for TororiError {}

pub type TororiResult<T> = Result<T, TororiError>;
