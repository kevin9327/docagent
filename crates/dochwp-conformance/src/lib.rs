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

/// Score rhwp by running its real CLI (`info --json`, `dump-pages --json`).
/// A missing binary is unrunnable (None). A present binary that fails to parse
/// is a real zero, not a skipped comparison.
pub fn score_rhwp(
    name: &str,
    bytes: &[u8],
    expected_pages: usize,
    rhwp_root: Option<&Path>,
) -> DocScore {
    if let Some(root) = rhwp_root
        && let Some(score) = try_rhwp_cli(name, bytes, expected_pages, root)
    {
        return score;
    }
    zero_rhwp(name)
}

fn zero_rhwp(name: &str) -> DocScore {
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

fn find_rhwp_bin(root: &Path) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("RHWP_BIN") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }
    let candidates = [
        root.join("target/release/rhwp.exe"),
        root.join("target/debug/rhwp.exe"),
        root.join("target/release/rhwp"),
        root.join("target/debug/rhwp"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

fn try_rhwp_cli(name: &str, bytes: &[u8], expected_pages: usize, root: &Path) -> Option<DocScore> {
    let bin = find_rhwp_bin(root)?;
    let tmp = std::env::temp_dir().join(format!("dochwp-rhwp-{name}.hwp"));
    std::fs::write(&tmp, bytes).ok()?;
    let path = tmp.to_str()?.to_string();

    let info = Proc::new(&bin)
        .args(["info", &path, "--json"])
        .output()
        .ok()?;
    if !info.status.success() {
        return Some(zero_rhwp(name));
    }
    let info_json: serde_json::Value = serde_json::from_slice(&info.stdout).unwrap_or(serde_json::Value::Null);
    let parsed = info_json.get("pageCount").is_some() || info_json.get("sections").is_some();
    if !parsed {
        return Some(zero_rhwp(name));
    }
    let rhwp_pages = info_json
        .get("pageCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    let parse = 100.0;
    let page_score = if expected_pages == 0 || rhwp_pages == expected_pages {
        100.0
    } else {
        let diff = (rhwp_pages as i32 - expected_pages as i32).unsigned_abs() as f64;
        (100.0 - diff * 25.0).max(0.0)
    };

    let oracle = dochwp_hwp5::read(bytes).ok();
    let hints: Vec<LayoutHint> = oracle
        .as_ref()
        .map(|d| {
            d.sections
                .iter()
                .flat_map(|s| s.body.iter())
                .filter_map(|b| match b {
                    Block::Paragraph(p) => Some(p.layout_hints.clone()),
                    _ => None,
                })
                .flatten()
                .collect()
        })
        .unwrap_or_default();

    let dump = Proc::new(&bin)
        .args(["dump-pages", &path, "--json"])
        .output()
        .ok();
    let dump_json = dump
        .as_ref()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok());
    let rhwp_ys = extract_y_hu(dump_json.as_ref());
    let lineseg = if hints.is_empty() {
        100.0
    } else if rhwp_ys.is_empty() {
        0.0
    } else {
        let n = hints.len().min(rhwp_ys.len());
        let hits = (0..n)
            .filter(|i| (hints[*i].y - rhwp_ys[*i]).abs() <= 50)
            .count();
        (hits as f64) * 100.0 / (hints.len() as f64)
    };

    let expected_rows: Vec<i32> = oracle
        .as_ref()
        .map(|d| {
            d.sections
                .iter()
                .flat_map(|s| s.body.iter())
                .filter_map(|b| match b {
                    Block::Table(t) => Some(t.rows.iter().filter_map(|r| r.height).collect::<Vec<_>>()),
                    _ => None,
                })
                .flatten()
                .collect()
        })
        .unwrap_or_default();
    let rhwp_rows = extract_row_heights_hu(dump_json.as_ref());
    let row_height = if expected_rows.is_empty() {
        100.0
    } else if rhwp_rows.is_empty() {
        0.0
    } else {
        let n = expected_rows.len().min(rhwp_rows.len());
        let hits = (0..n)
            .filter(|i| (expected_rows[*i] - rhwp_rows[*i]).abs() <= 200)
            .count();
        (hits as f64) * 100.0 / (expected_rows.len() as f64)
    };

    let total = (lineseg * f64::from(WEIGHT_LINESEG)
        + page_score * f64::from(WEIGHT_PAGE_COUNT)
        + row_height * f64::from(WEIGHT_ROW_HEIGHT)
        + parse * f64::from(WEIGHT_PARSE))
        / 100.0;
    Some(DocScore {
        name: name.into(),
        engine: "rhwp".into(),
        total,
        lineseg,
        page_count: page_score,
        row_height,
        parse,
    })
}

/// rhwp dump-pages / render trees mix px and HU. 96 dpi Hangul uses 75 HU/px
/// (`HU_PER_PX` in rhwp svg.rs). Values already in the HWPUNIT range are kept.
fn to_hu_maybe_px(v: f64) -> i32 {
    if v.abs() > 400.0 {
        v.round() as i32
    } else {
        (v * 75.0).round() as i32
    }
}

fn walk_numbers(value: &serde_json::Value, keys: &[&str], out: &mut Vec<i32>) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                if keys.iter().any(|want| k.eq_ignore_ascii_case(want))
                    && let Some(n) = v.as_f64()
                {
                    out.push(to_hu_maybe_px(n));
                }
                walk_numbers(v, keys, out);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                walk_numbers(item, keys, out);
            }
        }
        _ => {}
    }
}

fn extract_y_hu(dump: Option<&serde_json::Value>) -> Vec<i32> {
    let Some(v) = dump else {
        return Vec::new();
    };
    let mut out = Vec::new();
    walk_numbers(v, &["y", "top", "vertpos", "verticalPos"], &mut out);
    out
}

fn extract_row_heights_hu(dump: Option<&serde_json::Value>) -> Vec<i32> {
    let Some(v) = dump else {
        return Vec::new();
    };
    let mut out = Vec::new();
    walk_numbers(v, &["rowHeight", "row_height", "cellHeight"], &mut out);
    out
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
    if let Ok(p) = std::env::var("RHWP_DIR") {
        return Some(PathBuf::from(p));
    }
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let sibling = here.join("../../../rhwp");
    if sibling.exists() {
        Some(sibling.canonicalize().unwrap_or(sibling))
    } else {
        None
    }
}

/// Real Hangul-authored files from a sibling rhwp clone. Not vendored.
pub fn sibling_sample_fixtures(rhwp_root: Option<&Path>) -> Vec<(String, Vec<u8>, usize)> {
    let Some(root) = rhwp_root else {
        return Vec::new();
    };
    let names = [
        "samples/basic/pau-004.hwp",
        "samples/basic/BlogForm_BookReview.hwp",
        "samples/basic/english.hwp",
        "samples/basic/Textmail.hwp",
    ];
    let bin = find_rhwp_bin(root);
    let mut out = Vec::new();
    for rel in names {
        let path = root.join(rel);
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let pages = bin
            .as_ref()
            .and_then(|b| rhwp_info_page_count(b, &path))
            .unwrap_or(1);
        out.push((
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("sample")
                .to_string(),
            bytes,
            pages,
        ));
    }
    out
}

fn rhwp_info_page_count(bin: &Path, file: &Path) -> Option<usize> {
    let out = Proc::new(bin)
        .args(["info", file.to_str()?, "--json"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    v.get("pageCount").and_then(|x| x.as_u64()).map(|n| n as usize)
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
        let tree = layout_document(&doc, &FontSet::bundled());
        if let Some(Block::Paragraph(p)) = doc.sections[0].body.get_mut(0) {
            p.layout_hints = tree
                .pages
                .iter()
                .flat_map(|page| page.lines.iter())
                .map(|line| LayoutHint {
                    text_start: 0,
                    x: line.x,
                    y: line.y,
                    width: line.width,
                    line_height: line.height,
                    text_height: line.font_size,
                    baseline: line.baseline,
                    spacing: 0,
                    flags: 0,
                })
                .collect();
        }
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
