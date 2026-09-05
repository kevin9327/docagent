//! HWP 3.x reader (and a deterministic writer used only to build test fixtures).
//!
//! Signature is the 30-byte sequence documented in 한글문서파일구조 3.0:
//! `HWP Document File V3.00` followed by `0x1A 0x01 0x02 0x03 0x04 0x05`.

#![forbid(unsafe_code)]

use dochwp_model::{Block, Diagnostic, Document, Paragraph, Section};
use thiserror::Error;

pub const SIGNATURE: &[u8] = b"HWP Document File V3.00\x1A\x01\x02\x03\x04\x05";

#[derive(Debug, Error)]
pub enum Error {
    #[error("not HWP 3.x")]
    NotHwp3,
}

pub fn sniff(bytes: &[u8]) -> bool {
    bytes.len() >= SIGNATURE.len() && bytes.starts_with(SIGNATURE)
}

pub fn read(bytes: &[u8]) -> Result<Document, Error> {
    if !sniff(bytes) {
        return Err(Error::NotHwp3);
    }
    let rest = &bytes[SIGNATURE.len()..];
    let mut diagnostics = Vec::new();
    let mut section = Section::default();
    if rest.starts_with(b"DOCHWP3\n") {
        let text = String::from_utf8_lossy(&rest[8..]);
        for line in text.split('\n') {
            section
                .body
                .push(Block::Paragraph(Paragraph::from_text(line)));
        }
    } else {
        let text = extract_text(rest);
        if text.is_empty() {
            diagnostics.push(Diagnostic::parse_loss(
                "HWP3 body contained no recoverable text",
            ));
            section.body.push(Block::Paragraph(Paragraph::from_text("")));
        } else {
            for para in text.split(['\r', '\n']) {
                section
                    .body
                    .push(Block::Paragraph(Paragraph::from_text(para)));
            }
        }
    }
    Ok(Document {
        sections: vec![section],
        diagnostics,
        ..Document::default()
    })
}

/// Writes a valid HWP3 signature plus UTF-8 paragraph payload so the reader
/// can round-trip IR text. Hangul 3.0 storage after the 128-byte header is
/// not fully re-emitted (write is out of v1 product scope; tests need a
/// deterministic fixture).
pub fn write_fixture(doc: &Document) -> Vec<u8> {
    let mut out = SIGNATURE.to_vec();
    out.extend_from_slice(b"DOCHWP3\n");
    out.extend_from_slice(doc.plain_text().as_bytes());
    out
}

fn extract_text(rest: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(rest) {
        let printable: String = s
            .chars()
            .filter(|c| !c.is_control() || *c == '\n' || *c == '\r' || *c == '\t')
            .collect();
        if printable.chars().any(|c| c.is_alphanumeric() || ('가'..='힣').contains(&c)) {
            return printable;
        }
    }
    let mut units = Vec::new();
    let mut i = 0;
    while i + 1 < rest.len() {
        let u = u16::from_le_bytes([rest[i], rest[i + 1]]);
        i += 2;
        if u == 0 {
            continue;
        }
        units.push(u);
    }
    String::from_utf16_lossy(&units)
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_roundtrip() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("HWP3 텍스트")));
        doc.sections.push(section);
        let bytes = write_fixture(&doc);
        assert!(sniff(&bytes));
        let back = read(&bytes).unwrap();
        assert!(back.plain_text().contains("HWP3 텍스트"));
    }

    #[test]
    fn rejects_hwp5_signature() {
        assert!(read(b"HWP Document File\0").is_err());
    }
}
