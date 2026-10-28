//! Small shared helpers: hashing, bounded pagination, argument
//! extraction. Everything here is deterministic and allocation-
//! bounded; list bounds are the tiger-style-mcp contract (every
//! list bounded and paginated, partial results declared).

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::error::{TororiError, TororiResult};

/// Default page size for list tools.
pub const PAGE_DEFAULT: usize = 50;
/// Hard page-size ceiling for list tools.
pub const PAGE_MAX: usize = 200;

/// sha256 of a byte slice, lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// sha256 of a file's bytes, lowercase hex.
pub fn sha256_file(path: &std::path::Path) -> TororiResult<String> {
    let bytes = std::fs::read(path)
        .map_err(|e| TororiError::new("io", format!("cannot read {}: {e}", path.display())))?;
    Ok(sha256_hex(&bytes))
}

/// Parsed pagination arguments shared by every list tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    pub offset: usize,
    pub limit: usize,
}

impl Page {
    pub fn parse(args: &Value) -> TororiResult<Page> {
        let offset = match args.get("offset") {
            None | Some(Value::Null) => 0,
            Some(v) => v.as_u64().ok_or_else(|| {
                TororiError::invalid_input("offset must be a non-negative integer")
            })? as usize,
        };
        let limit = match args.get("limit") {
            None | Some(Value::Null) => PAGE_DEFAULT,
            Some(v) => {
                let n = v
                    .as_u64()
                    .ok_or_else(|| TororiError::invalid_input("limit must be a positive integer"))?
                    as usize;
                if n == 0 {
                    return Err(TororiError::invalid_input("limit must be at least 1"));
                }
                n.min(PAGE_MAX)
            }
        };
        Ok(Page { offset, limit })
    }

    /// Slice `items` to this page and build the standard list
    /// envelope: the page, the server-computed total, and the
    /// `truncated` declaration when results remain.
    pub fn apply<T: serde::Serialize>(&self, items: &[T]) -> Value {
        let total = items.len();
        let page: Vec<&T> = items.iter().skip(self.offset).take(self.limit).collect();
        let returned = page.len();
        json!({
            "items": serde_json::to_value(&page).unwrap_or(Value::Null),
            "total": total,
            "offset": self.offset,
            "limit": self.limit,
            "returned": returned,
            "truncated": self.offset + returned < total,
        })
    }
}

/// Required string argument.
pub fn arg_str<'a>(args: &'a Value, field: &str) -> TororiResult<&'a str> {
    args.get(field)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| TororiError::invalid_input(format!("{field} is required (string)")))
}

/// Optional string argument.
pub fn opt_str<'a>(args: &'a Value, field: &str) -> Option<&'a str> {
    args.get(field)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

/// Required argument of any JSON shape, cloned.
pub fn arg_value(args: &Value, field: &str) -> TororiResult<Value> {
    args.get(field)
        .filter(|v| !v.is_null())
        .cloned()
        .ok_or_else(|| TororiError::invalid_input(format!("{field} is required")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn page_bounds_and_truncation() {
        let page = Page::parse(&json!({"limit": 9999})).expect("parse");
        assert_eq!(page.limit, PAGE_MAX);
        let items: Vec<i32> = (0..10).collect();
        let out = Page {
            offset: 8,
            limit: 5,
        }
        .apply(&items);
        assert_eq!(out["total"], 10);
        assert_eq!(out["returned"], 2);
        assert_eq!(out["truncated"], false);
        let out = Page {
            offset: 0,
            limit: 3,
        }
        .apply(&items);
        assert_eq!(out["truncated"], true);
    }

    #[test]
    fn page_rejects_bad_shapes() {
        assert!(Page::parse(&json!({"limit": 0})).is_err());
        assert!(Page::parse(&json!({"offset": -1})).is_err());
        assert!(Page::parse(&json!({"limit": "x"})).is_err());
    }
}
