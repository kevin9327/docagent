//! Real-font shaping. No hardcoded glyph-advance tables.

#![forbid(unsafe_code)]

use std::sync::OnceLock;

use docagent_model::{Diagnostic, DiagnosticCode, Document, FontFace, Severity};
use rustybuzz::{Face, UnicodeBuffer};
use thiserror::Error;
use ttf_parser::Face as TtfFace;

pub const BUNDLED_FONT: &[u8] = include_bytes!("../../../fonts/NotoSans-Regular.ttf");

#[derive(Debug, Error)]
pub enum Error {
    #[error("font parse failed")]
    Parse,
}

#[derive(Clone, Debug)]
pub struct FontSet {
    pub data: Vec<u8>,
    pub fingerprint: u64,
}

impl FontSet {
    pub fn bundled() -> Self {
        static CELL: OnceLock<FontSet> = OnceLock::new();
        CELL.get_or_init(|| FontSet {
            data: BUNDLED_FONT.to_vec(),
            fingerprint: hash_bytes(BUNDLED_FONT),
        })
        .clone()
    }

    pub fn from_bytes(data: Vec<u8>) -> Result<Self, Error> {
        TtfFace::parse(&data, 0).map_err(|_| Error::Parse)?;
        let fingerprint = hash_bytes(&data);
        Ok(Self { data, fingerprint })
    }

    pub fn units_per_em(&self) -> i32 {
        TtfFace::parse(&self.data, 0)
            .map(|f| i32::from(f.units_per_em()))
            .unwrap_or(1000)
    }
}

fn hash_bytes(data: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for b in data {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

#[derive(Clone, Debug)]
pub struct ShapedRun {
    pub text: String,
    pub advances: Vec<i32>,
    pub glyph_ids: Vec<u16>,
    pub width: i32,
}

/// Shape `text` at `size_hu` using the real font file.
/// Advances are HWPUNIT via (font_unit * size_hu) / units_per_em.
pub fn shape(font: &FontSet, text: &str, size_hu: i32) -> ShapedRun {
    if text.is_empty() {
        return ShapedRun {
            text: String::new(),
            advances: Vec::new(),
            glyph_ids: Vec::new(),
            width: 0,
        };
    }
    let Some(face) = Face::from_slice(&font.data, 0) else {
        return fallback_shape(text, size_hu);
    };
    let upem = i64::from(face.units_per_em().max(1));
    let mut buf = UnicodeBuffer::new();
    buf.push_str(text);
    let glyphs = rustybuzz::shape(&face, &[], buf);
    let mut advances = Vec::with_capacity(glyphs.len());
    let mut glyph_ids = Vec::with_capacity(glyphs.len());
    let mut width: i64 = 0;
    let size = i64::from(size_hu.max(1));
    for (info, pos) in glyphs.glyph_infos().iter().zip(glyphs.glyph_positions()) {
        let adv = (i64::from(pos.x_advance) * size) / upem;
        let adv_i32 = adv.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        advances.push(adv_i32);
        glyph_ids.push(info.glyph_id as u16);
        width += i64::from(adv_i32);
    }
    ShapedRun {
        text: text.to_string(),
        advances,
        glyph_ids,
        width: width.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
    }
}

/// Named rule: a missing-glyph cluster occupies the em square.
/// Spec basis: OpenType `hmtx` missing-glyph behaviour, not a sample-tuned value.
const MISSING_GLYPH_EM_NUMERATOR: i32 = 1;

fn fallback_shape(text: &str, size_hu: i32) -> ShapedRun {
    let adv = size_hu.saturating_mul(MISSING_GLYPH_EM_NUMERATOR);
    let n = text.chars().count();
    ShapedRun {
        text: text.to_string(),
        advances: vec![adv; n],
        glyph_ids: vec![0; n],
        width: adv.saturating_mul(n as i32),
    }
}

pub fn resolve_for_document(doc: &Document, fonts: &FontSet) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    if doc.fonts.iter().any(|FontFace { name, .. }| {
        !name.is_empty() && name != "Noto Sans" && name != "함초롬돋움"
    }) {
        out.push(Diagnostic {
            severity: Severity::Info,
            code: DiagnosticCode::FontSubstitute,
            message: format!(
                "requested faces substituted with bundled font fingerprint {:x}",
                fonts.fingerprint
            ),
            location: None,
        });
    }
    out
}

/// Drop characters the bundled face cannot map. PDF/A forbids .notdef glyphs.
pub fn retain_mapped_chars(font: &FontSet, text: &str) -> String {
    let Ok(face) = TtfFace::parse(&font.data, 0) else {
        return text.to_string();
    };
    text.chars()
        .filter(|c| c.is_whitespace() || face.glyph_index(*c).is_some())
        .collect()
}

pub fn line_height(font: &FontSet, size_hu: i32, spacing_percent: u16) -> i32 {
    let face = TtfFace::parse(&font.data, 0).ok();
    let upem = i64::from(face.as_ref().map(|f| f.units_per_em()).unwrap_or(1000).max(1));
    let (asc, desc) = face
        .map(|f| (i64::from(f.ascender()), i64::from(f.descender().abs())))
        .unwrap_or((800, 200));
    let em = ((asc + desc) * i64::from(size_hu)).div_euclid(upem);
    let pct = i64::from(spacing_percent.max(100));
    ((em * pct) / 100).clamp(1, i64::from(i32::MAX)) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_font_parses() {
        let set = FontSet::bundled();
        assert!(set.units_per_em() > 0);
        let shaped = shape(&set, "Hello", 1000);
        assert!(shaped.width > 0);
        assert_eq!(shaped.advances.len(), 5);
    }

    #[test]
    fn shaping_is_deterministic() {
        let set = FontSet::bundled();
        let a = shape(&set, "DocAgent", 1200);
        let b = shape(&set, "DocAgent", 1200);
        assert_eq!(a.advances, b.advances);
        assert_eq!(a.width, b.width);
    }
}
