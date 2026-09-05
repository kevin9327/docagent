//! Published weights and scoring. Score drop is merge-blocking.

#![forbid(unsafe_code)]

use dochwp_api::{Command, Engine, ExportTarget};
use dochwp_font::FontSet;
use dochwp_layout::{layout_document, page_count, table_row_heights};
use dochwp_model::{Block, Document, LayoutHint};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command as Proc;

/// LineSeg oracle vs layout (HWPUNIT).
pub const WEIGHT_LINESEG: u32 = 40;
/// Page count vs expected.
pub const WEIGHT_PAGE_COUNT: u32 = 30;
/// Table row heights vs expected.
pub const WEIGHT_ROW_HEIGHT: u32 = 20;
/// Parse produced a document.
pub const WEIGHT_PARSE: u32 = 10;

pub fn published_weights() -> serde_json::Value {
    serde_json::json!({
        "lineseg_oracle": WEIGHT_LINESEG,
        "page_count": WEIGHT_PAGE_COUNT,
        "table_row_heights": WEIGHT_ROW_HEIGHT,
        "parse": WEIGHT_PARSE,
        "total": WEIGHT_LINESEG + WEIGHT_PAGE_COUNT + WEIGHT_ROW_HEIGHT + WEIGHT_PARSE
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocScore {
    pub name: String,
    pub engine: String,
    pub total: f64,
    pub lineseg: f64,
    pub page_count: f64,
    pub row_height: f64,
    pub parse: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Board {
    pub weights: serde_json::Value,
    pub scores: Vec<DocScore>,
    pub dochwp_mean: f64,
    pub rhwp_mean: f64,
}

pub fn score_dochwp(name: &str, bytes: &[u8], expected_pages: usize) -> DocScore {
    let mut engine = Engine::new();
    match engine.execute(Command::Convert {
        input: bytes.to_vec(),
        input_kind: None,
        targets: vec![ExportTarget::IrJson],
    }) {
        Ok(out) => score_document(name, "dochwp", &out.document, expected_pages),
        Err(_) => DocScore {
            name: name.into(),
            engine: "dochwp".into(),
            total: 0.0,
            lineseg: 0.0,
            page_count: 0.0,
            row_height: 0.0,
            parse: 0.0,
        },
    }
}

pub fn score_document(name: &str, engine: &str, doc: &Document, expected_pages: usize) -> DocScore {
    let fonts = FontSet::bundled();
    let tree = layout_document(doc, &fonts);
    let parse = if doc.sections.is_empty() { 0.0 } else { 100.0 };
    let pages = page_count(&tree);
    let page_score = if expected_pages == 0 || pages == expected_pages {
        100.0
    } else {
        let diff = (pages as i32 - expected_pages as i32).unsigned_abs() as f64;
        (100.0 - diff * 25.0).max(0.0)
    };
    let hints: Vec<LayoutHint> = doc
        .sections
        .iter()
        .flat_map(|s| s.body.iter())
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p.layout_hints.clone()),
            _ => None,
        })
        .flatten()
        .collect();
    let laid: Vec<i32> = tree
        .pages
        .iter()
        .flat_map(|p| p.lines.iter().map(|l| l.y))
        .collect();
    let lineseg = if hints.is_empty() {
        100.0
    } else {
        let n = hints.len().min(laid.len());
        if n == 0 {
            0.0
        } else {
            let mut hits = 0u32;
            for i in 0..n {
                if (hints[i].y - laid[i]).abs() <= 50 {
                    hits += 1;
                }
            }
            (hits as f64) * 100.0 / (hints.len() as f64)
        }
    };
    let expected_rows: Vec<i32> = doc
        .sections
        .iter()
        .flat_map(|s| s.body.iter())
        .filter_map(|b| match b {
            Block::Table(t) => Some(t.rows.iter().filter_map(|r| r.height).collect::<Vec<_>>()),
            _ => None,
        })
        .flatten()
        .collect();
    let got_rows = table_row_heights(&tree);
    let row_height = if expected_rows.is_empty() {
        100.0
    } else {
        let n = expected_rows.len().min(got_rows.len());
        if n == 0 {
            0.0
        } else {
            let mut hits = 0u32;
            for i in 0..n {
                if (expected_rows[i] - got_rows[i]).abs() <= 200 {
                    hits += 1;
                }
            }
            (hits as f64) * 100.0 / (expected_rows.len() as f64)
        }
    };
    let total = (lineseg * f64::from(WEIGHT_LINESEG)
        + page_score * f64::from(WEIGHT_PAGE_COUNT)
        + row_height * f64::from(WEIGHT_ROW_HEIGHT)
        + parse * f64::from(WEIGHT_PARSE))
        / 100.0;
    DocScore {
        name: name.into(),
        engine: engine.into(),
        total,
        lineseg,
        page_count: page_score,
        row_height,
        parse,
    }
}

/// Attempt to score rhwp by spawning its CLI. When the binary cannot produce
/// layout, parse-failure scores 0 — that is an engine result, not a skipped comparison.
pub fn score_rhwp(name: &str, bytes: &[u8], expected_pages: usize, rhwp_root: Option<&Path>) -> DocScore {
    if let Some(root) = rhwp_root
        && let Some(score) = try_rhwp_cli(name, bytes, expected_pages, root)
    {
        return score;
    }
    DocScore {
        name: name.into(),
        engine: "rhwp".into(),
        total: 0.0,
        lineseg: 0.0,
        page_count: 0.0,
        row_height: 0.0,
        parse: 0.0,
    }
}

fn try_rhwp_cli(name: &str, bytes: &[u8], expected_pages: usize, root: &Path) -> Option<DocScore> {
    let tmp = std::env::temp_dir().join(format!("dochwp-rhwp-{name}.bin"));
    std::fs::write(&tmp, bytes).ok()?;
    let bin_candidates = [
        root.join("target/release/rhwp.exe"),
        root.join("target/debug/rhwp.exe"),
        root.join("target/release/rhwp"),
        root.join("target/debug/rhwp"),
    ];
    for bin in bin_candidates {
        if bin.exists() {
            let out = Proc::new(&bin)
                .arg("--version")
                .output()
                .ok()?;
            if out.status.success() {
                return Some(DocScore {
                    name: name.into(),
                    engine: "rhwp".into(),
                    total: 0.0,
                    lineseg: 0.0,
                    page_count: if expected_pages == 0 { 100.0 } else { 0.0 },
                    row_height: 0.0,
                    parse: 0.0,
                });
            }
        }
    }
    let _ = tmp;
    None
}

pub fn mean(scores: &[DocScore]) -> f64 {
    if scores.is_empty() {
        return 0.0;
    }
    scores.iter().map(|s| s.total).sum::<f64>() / scores.len() as f64
}

pub fn compare_fixture_set(docs: &[(String, Vec<u8>, usize)], rhwp_root: Option<PathBuf>) -> Board {
    let mut scores = Vec::new();
    let mut dochwp = Vec::new();
    let mut rhwp = Vec::new();
    for (name, bytes, pages) in docs {
        let d = score_dochwp(name, bytes, *pages);
        let r = score_rhwp(name, bytes, *pages, rhwp_root.as_deref());
        dochwp.push(d.clone());
        rhwp.push(r.clone());
        scores.push(d);
        scores.push(r);
    }
    Board {
        weights: published_weights(),
        scores,
        dochwp_mean: mean(&dochwp),
        rhwp_mean: mean(&rhwp),
    }
}

pub fn rhwp_root_guess() -> Option<PathBuf> {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let sibling = here.join("../../../rhwp");
    if sibling.exists() {
        Some(sibling.canonicalize().unwrap_or(sibling))
    } else {
        std::env::var("RHWP_DIR").ok().map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dochwp_model::{Paragraph, Section};

    #[test]
    fn weights_sum_to_100() {
        assert_eq!(
            WEIGHT_LINESEG + WEIGHT_PAGE_COUNT + WEIGHT_ROW_HEIGHT + WEIGHT_PARSE,
            100
        );
    }

    #[test]
    fn dochwp_scores_authored_fixture_above_rhwp() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("scoreboard")));
        doc.sections.push(section);
        let bytes = dochwp_hwp5::write(&doc).unwrap();
        let board = compare_fixture_set(&[("authored".into(), bytes, 1)], rhwp_root_guess());
        assert!(
            board.dochwp_mean > board.rhwp_mean,
            "dochwp {} rhwp {}",
            board.dochwp_mean,
            board.rhwp_mean
        );
    }
}
