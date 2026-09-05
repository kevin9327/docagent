//! Community codec template (ODT / RTF / Markdown).
//!
//! Copy this crate, rename it, depend only on `dochwp-model`, implement
//! `sniff` / `read` / `write`. Do not import layout or other codecs.
//! See `docs/src/codec-guide.md`.

#![forbid(unsafe_code)]

use dochwp_model::Document;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("not implemented")]
    NotImplemented,
}

pub fn sniff(_bytes: &[u8]) -> bool {
    false
}

pub fn read(_bytes: &[u8]) -> Result<Document, Error> {
    Err(Error::NotImplemented)
}

pub fn write(_doc: &Document) -> Result<Vec<u8>, Error> {
    Err(Error::NotImplemented)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_does_not_claim_unknown_bytes() {
        assert!(!sniff(b"hello"));
        assert!(read(b"hello").is_err());
    }
}
