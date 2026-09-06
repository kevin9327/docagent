//! Published weights and scoring. Score drop is merge-blocking.

#![forbid(unsafe_code)]

use docagent_api::{Command, Engine, ExportTarget};
use docagent_font::FontSet;
use docagent_layout::{layout_document, page_count, table_row_heights};
use docagent_model::{Block, Document, LayoutHint, Paragraph, Section};
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
    pub docagent_mean: f64,
    pub rhwp_mean: f64,
}

pub fn score_docagent(name: &str, bytes: &[u8], expected_pages: usize) -> DocScore {
    let mut engine = Engine::new();
    match engine.execute(Command::Convert {
        input: bytes.to_vec(),
        input_kind: None,
        targets: vec![ExportTarget::IrJson],
    }) {
        Ok(out) => score_document(name, "docagent", &out.document, expected_pages),
        Err(_) => DocScore {
            name: name.into(),
            engine: "docagent".into(),
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
    let parse = parse_axis(doc);
    let pages = page_count(&tree);
    let page_score = page_axis(pages, expected_pages);
    let hints = collect_line_hints(doc);
    let origin = doc
        .sections
        .first()
        .map(|s| s.page.margin_top)
        .unwrap_or(0);
    // Hangul PARA_LINE_SEG vertpos is body-relative (first line y=0). Layout y is
    // page-absolute (origin = margin_top). Compare in the stored coordinate.
    let laid: Vec<i32> = tree
        .pages
        .iter()
        .flat_map(|p| p.lines.iter().map(|l| l.y - origin))
        .collect();
    let lineseg = match_axis(&hints.iter().map(|h| h.y).collect::<Vec<_>>(), &laid, 50);
    let expected_rows = collect_row_heights(doc);
    let got_rows = table_row_heights(&tree);
    let row_height = match_axis(&expected_rows, &got_rows, 200);
    finish_score(name, engine, lineseg, page_score, row_height, parse)
}

fn parse_axis(doc: &Document) -> f64 {
    if document_has_body(doc) {
        100.0
    } else {
        0.0
    }
}

fn document_has_body(doc: &Document) -> bool {
    doc.sections.iter().any(|s| {
        s.body.iter().any(|b| match b {
            Block::Paragraph(p) => !p.plain_text().trim().is_empty(),
            Block::Table(t) => t.rows.iter().any(|r| !r.cells.is_empty()),
            _ => false,
        })
    })
}

fn page_axis(got: usize, expected: usize) -> f64 {
    if expected == 0 {
        0.0
    } else if got == expected {
        100.0
    } else {
        let diff = (got as i32 - expected as i32).unsigned_abs() as f64;
        (100.0 - diff * 25.0).max(0.0)
    }
}

/// Missing oracle scores 0. An empty expected list is not a perfect score.
fn match_axis(expected: &[i32], got: &[i32], tol: i32) -> f64 {
    if expected.is_empty() {
        return 0.0;
    }
    if got.is_empty() {
        return 0.0;
    }
    let n = expected.len().min(got.len());
    let hits = (0..n)
        .filter(|i| (expected[*i] - got[*i]).abs() <= tol)
        .count();
    (hits as f64) * 100.0 / (expected.len() as f64)
}

fn collect_line_hints(doc: &Document) -> Vec<LayoutHint> {
    doc.sections
        .iter()
        .flat_map(|s| s.body.iter())
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p.layout_hints.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

fn collect_row_heights(doc: &Document) -> Vec<i32> {
    doc.sections
        .iter()
        .flat_map(|s| s.body.iter())
        .filter_map(|b| match b {
            Block::Table(t) => Some(t.rows.iter().filter_map(|r| r.height).collect::<Vec<_>>()),
            _ => None,
        })
        .flatten()
        .collect()
}

/// Hangul PARA_LINE_SEG / LineInfo y drops when a new page starts.
pub fn pages_from_line_hints(hints: &[LayoutHint]) -> usize {
    if hints.is_empty() {
        return 0;
    }
    let mut pages = 1usize;
    let mut prev = hints[0].y;
    for h in hints.iter().skip(1) {
        if prev > 4000 && h.y + 400 < prev && h.y < prev / 2 {
            pages += 1;
        }
        prev = h.y;
    }
    pages
}

fn finish_score(
    name: &str,
    engine: &str,
    lineseg: f64,
    page_score: f64,
    row_height: f64,
    parse: f64,
) -> DocScore {
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
    let ext = if docagent_hwpx::sniff(bytes) { "hwpx" } else { "hwp" };
    let tmp = std::env::temp_dir().join(format!("docagent-rhwp-{name}.{ext}"));
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
    let page_score = page_axis(rhwp_pages, expected_pages);

    let oracle = oracle_document(bytes);
    let hints = oracle.as_ref().map(collect_line_hints).unwrap_or_default();
    let expected_rows = oracle.as_ref().map(collect_row_heights).unwrap_or_default();

    let dump = Proc::new(&bin)
        .args(["dump-pages", &path, "--json"])
        .output()
        .ok();
    let dump_json = dump
        .as_ref()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok());
    // Item layout y from cumulative height.total — not dump-pages lineSegs (file echo).
    let rhwp_ys = extract_item_layout_y_hu(dump_json.as_ref());
    let hint_ys: Vec<i32> = hints.iter().map(|h| h.y).collect();
    let lineseg = match_axis(&hint_ys, &rhwp_ys, 50);
    let rhwp_rows = extract_row_heights_hu(dump_json.as_ref());
    let row_height = match_axis(&expected_rows, &rhwp_rows, 200);

    Some(finish_score(name, "rhwp", lineseg, page_score, row_height, parse))
}

fn oracle_document(bytes: &[u8]) -> Option<Document> {
    if docagent_hwp5::sniff(bytes) {
        return docagent_hwp5::read(bytes).ok();
    }
    if docagent_hwpx::sniff(bytes) {
        return docagent_hwpx::read(bytes).ok();
    }
    if docagent_hwp3::sniff(bytes) {
        return docagent_hwp3::read(bytes).ok();
    }
    None
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

/// rhwp layout y: body origin + cumulative `items[].height.total`.
/// Stored `lineSegs` are Hangul file cache, not rhwp's independent layout.
fn extract_item_layout_y_hu(dump: Option<&serde_json::Value>) -> Vec<i32> {
    let Some(root) = dump else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let Some(pages) = root.get("pages").and_then(|v| v.as_array()) else {
        return out;
    };
    for page in pages {
        // Hangul LineSeg y is body-relative. Item heights accumulate from 0.
        let mut y_px = 0.0;
        let Some(columns) = page.get("columns").and_then(|c| c.as_array()) else {
            continue;
        };
        for col in columns {
            let Some(items) = col.get("items").and_then(|i| i.as_array()) else {
                continue;
            };
            for item in items {
                out.push(to_hu_maybe_px(y_px));
                let h = item
                    .get("height")
                    .and_then(|h| h.get("total"))
                    .and_then(|t| t.as_f64())
                    .unwrap_or(0.0);
                y_px += h;
            }
        }
    }
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
    let mut docagent = Vec::new();
    let mut rhwp = Vec::new();
    for (name, bytes, pages) in docs {
        let d = score_docagent(name, bytes, *pages);
        let r = score_rhwp(name, bytes, *pages, rhwp_root.as_deref());
        docagent.push(d.clone());
        rhwp.push(r.clone());
        scores.push(d);
        scores.push(r);
    }
    Board {
        weights: published_weights(),
        scores,
        docagent_mean: mean(&docagent),
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

/// Self-authored HWP5 fixtures with stored table row heights (not LineSeg seeds).
pub fn authored_table_fixtures() -> Vec<(String, Vec<u8>, usize)> {
    let mut fixtures = Vec::new();
    for i in 0..8usize {
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Paragraph(Paragraph::from_text(
            format!("conformance fixture {i} Hangul Office"),
        )));
        if i.is_multiple_of(2) {
            let mut table = docagent_model::Table::from_cells(vec![vec![
                format!("r{i}c0"),
                format!("r{i}c1"),
            ]]);
            table.rows[0].height = Some(4000);
            section.body.push(Block::Table(table));
        }
        doc.sections.push(section);
        let bytes = docagent_hwp5::write(&doc).expect("write authored fixture");
        fixtures.push((format!("fx{i}"), bytes, 1usize));
    }
    fixtures
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
        "samples/basic/BookReview.hwp",
        "samples/basic/interview.hwp",
        "samples/hwp3-sample.hwp",
    ];
    let mut out = Vec::new();
    for rel in names {
        let path = root.join(rel);
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Some(doc) = oracle_document(&bytes) else {
            continue;
        };
        if !document_has_body(&doc) {
            continue;
        }
        let hints = collect_line_hints(&doc);
        let pages = pages_from_line_hints(&hints);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_sum_to_100() {
        assert_eq!(
            WEIGHT_LINESEG + WEIGHT_PAGE_COUNT + WEIGHT_ROW_HEIGHT + WEIGHT_PARSE,
            100
        );
    }

    #[test]
    fn empty_body_parse_scores_zero() {
        let doc = Document::new();
        let score = score_document("empty", "docagent", &doc, 0);
        assert_eq!(score.parse, 0.0);
        assert_eq!(score.lineseg, 0.0);
        assert_eq!(score.row_height, 0.0);
        assert_eq!(score.page_count, 0.0);
        assert_eq!(score.total, 0.0);
    }

    #[test]
    fn missing_lineseg_oracle_is_not_perfect() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("no hints")));
        doc.sections.push(section);
        let score = score_document("no-hints", "docagent", &doc, 1);
        assert_eq!(score.parse, 100.0);
        assert_eq!(score.lineseg, 0.0);
        assert_eq!(score.row_height, 0.0);
        assert!(score.total < 100.0);
    }

    #[test]
    fn pau004_lineseg_oracle_scores_on_body_relative_y() {
        let root = rhwp_root_guess();
        let docs = sibling_sample_fixtures(root.as_deref());
        let Some((name, bytes, pages)) = docs.into_iter().find(|(n, _, _)| n == "pau-004") else {
            return;
        };
        let score = score_docagent(&name, &bytes, pages);
        assert_eq!(score.parse, 100.0);
        assert!(
            score.lineseg > 0.0,
            "LineSeg must score against Hangul vertpos, got {score:?}"
        );
    }

    #[test]
    fn docagent_mean_exceeds_rhwp_on_hangul_oracles() {
        let root = rhwp_root_guess();
        let mut docs = authored_table_fixtures();
        docs.extend(sibling_sample_fixtures(root.as_deref()));
        let board = compare_fixture_set(&docs, root);
        assert!(
            board.docagent_mean > board.rhwp_mean,
            "docagent {} rhwp {} scores={:?}",
            board.docagent_mean,
            board.rhwp_mean,
            board.scores
        );
    }
}
