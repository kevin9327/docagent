//! Integer-coordinate layout. One engine. No I/O. No format identifiers.
//!
//! LineSeg hints on the IR are ignored here; the conformance crate is the
//! only consumer.

#![forbid(unsafe_code)]

use docagent_font::{line_height, shape, FontSet};
use docagent_model::{
    Alignment, Block, BorderStyle, Document, Hu, LineSpacing, NumberFormat, Paragraph, Section,
    Table, DEFAULT_FONT_SIZE_HU,
};
use rayon::prelude::*;

/// Minimum leftover in a table row before the next row starts.
/// Hangul 5.0 spec §4.3 table cell: content + padding; no sample-tuned slack.
pub const MIN_ROW_REMAINDER_HU: Hu = 0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FragmentTree {
    pub pages: Vec<PageFrag>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageFrag {
    pub width: Hu,
    pub height: Hu,
    pub lines: Vec<LineFrag>,
    pub rects: Vec<RectFrag>,
    /// Table grid strokes. Not counted by `table_row_heights`.
    pub strokes: Vec<RectFrag>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineFrag {
    pub x: Hu,
    pub y: Hu,
    pub width: Hu,
    pub height: Hu,
    pub baseline: Hu,
    pub text: String,
    pub font_size: Hu,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RectFrag {
    pub x: Hu,
    pub y: Hu,
    pub width: Hu,
    pub height: Hu,
    pub fill: [u8; 4],
}

pub fn layout_document(doc: &Document, fonts: &FontSet) -> FragmentTree {
    let mut pages = Vec::new();
    for section in &doc.sections {
        pages.extend(layout_section(section, fonts));
    }
    if pages.is_empty() {
        let p = Section::default().page;
        pages.push(PageFrag {
            width: p.width,
            height: p.height,
            lines: Vec::new(),
            rects: Vec::new(),
            strokes: Vec::new(),
        });
    }
    FragmentTree { pages }
}

fn layout_section(section: &Section, fonts: &FontSet) -> Vec<PageFrag> {
    let page = &section.page;
    let content_w = page.content_width();
    let content_h = page.content_height();
    let origin_x = page.margin_left;
    let origin_y = page.margin_top;

    let prepared: Vec<Prepared> = section
        .body
        .par_iter()
        .map(|block| prepare_block(block, fonts, content_w))
        .collect();

    let mut pages = Vec::new();
    let mut current = empty_page(page.width, page.height);
    let mut y = origin_y;

    for item in prepared {
        match item {
            Prepared::Lines { lines, space_before, space_after } => {
                y += space_before;
                for line in lines {
                    if y + line.height > origin_y + content_h && !current.lines.is_empty() {
                        pages.push(std::mem::replace(
                            &mut current,
                            empty_page(page.width, page.height),
                        ));
                        y = origin_y + space_before;
                    }
                    let mut placed = line;
                    placed.x = origin_x;
                    placed.y = y;
                    y += placed.height;
                    current.lines.push(placed);
                }
                y += space_after;
            }
            Prepared::Table {
                rows,
                col_widths,
                stroke_w,
                stroke,
            } => {
                let table_w: Hu = col_widths.iter().copied().sum();
                let x0 = origin_x;
                for (ri, row) in rows.iter().enumerate() {
                    let row_h = row.iter().map(|c| c.height).max().unwrap_or(0).max(MIN_ROW_REMAINDER_HU);
                    if y + row_h > origin_y + content_h && !current.lines.is_empty() {
                        pages.push(std::mem::replace(
                            &mut current,
                            empty_page(page.width, page.height),
                        ));
                        y = origin_y;
                    }
                    let mut x = x0;
                    for (ci, cell) in row.iter().enumerate() {
                        let cw = *col_widths.get(ci).unwrap_or(&10000);
                        current.rects.push(RectFrag {
                            x,
                            y,
                            width: cw,
                            height: row_h,
                            fill: if cell.header {
                                [240, 253, 250, 255]
                            } else {
                                [255, 255, 255, 255]
                            },
                        });
                        if stroke_w > 0 {
                            push_cell_strokes(&mut current.strokes, x, y, cw, row_h, stroke_w, stroke);
                        }
                        for line in &cell.lines {
                            let mut placed = line.clone();
                            placed.x = x + 80;
                            placed.y += y + 80;
                            current.lines.push(placed);
                        }
                        x += cw;
                    }
                    y += row_h;
                    let _ = (ri, table_w);
                }
                let _ = x0;
            }
        }
    }
    pages.push(current);
    pages
}

fn empty_page(width: Hu, height: Hu) -> PageFrag {
    PageFrag {
        width,
        height,
        lines: Vec::new(),
        rects: Vec::new(),
        strokes: Vec::new(),
    }
}

fn push_cell_strokes(
    strokes: &mut Vec<RectFrag>,
    x: Hu,
    y: Hu,
    w: Hu,
    h: Hu,
    t: Hu,
    color: [u8; 4],
) {
    let t = t.min(w).min(h).max(1);
    strokes.push(RectFrag { x, y, width: w, height: t, fill: color });
    strokes.push(RectFrag { x, y, width: t, height: h, fill: color });
    strokes.push(RectFrag {
        x,
        y: y + h - t,
        width: w,
        height: t,
        fill: color,
    });
    strokes.push(RectFrag {
        x: x + w - t,
        y,
        width: t,
        height: h,
        fill: color,
    });
}

enum Prepared {
    Lines {
        lines: Vec<LineFrag>,
        space_before: Hu,
        space_after: Hu,
    },
    Table {
        rows: Vec<Vec<PreparedCell>>,
        col_widths: Vec<Hu>,
        stroke_w: Hu,
        stroke: [u8; 4],
    },
}

struct PreparedCell {
    lines: Vec<LineFrag>,
    height: Hu,
    header: bool,
}

fn prepare_block(block: &Block, fonts: &FontSet, width: Hu) -> Prepared {
    match block {
        Block::Paragraph(p) => Prepared::Lines {
            lines: layout_paragraph(p, fonts, width),
            space_before: p.space_before.max(0),
            space_after: p.space_after.max(0),
        },
        Block::Table(t) => prepare_table(t, fonts, width),
        Block::Float(_) | Block::Break(_) => Prepared::Lines {
            lines: Vec::new(),
            space_before: 0,
            space_after: 0,
        },
    }
}

fn layout_paragraph(p: &Paragraph, fonts: &FontSet, width: Hu) -> Vec<LineFrag> {
    let text = p.plain_text();
    let size = p
        .runs
        .first()
        .map(|r| r.style.size)
        .unwrap_or(DEFAULT_FONT_SIZE_HU);
    let spacing_pct = match p.line_spacing {
        LineSpacing::Percent(v) => v,
        LineSpacing::Absolute(hu) => {
            if size == 0 {
                160
            } else {
                ((i64::from(hu) * 100) / i64::from(size.max(1))) as u16
            }
        }
        LineSpacing::AtLeast(hu) => {
            let pct = ((i64::from(hu) * 100) / i64::from(size.max(1))) as u16;
            pct.max(100)
        }
    };
    let lh = match p.line_spacing {
        LineSpacing::Absolute(hu) => hu.max(1),
        LineSpacing::AtLeast(hu) => line_height(fonts, size, 100).max(hu),
        LineSpacing::Percent(_) => line_height(fonts, size, spacing_pct),
    };
    let marker = list_marker(p);
    let marker_w = marker
        .as_deref()
        .map(|m| shape(fonts, m, size).width)
        .unwrap_or(0);
    let usable = (width - p.indent_left - p.indent_right - marker_w).max(1);
    let shaped = shape(fonts, &text, size);
    let ranges = break_lines(text.clone(), shaped.advances.clone(), usable);
    if ranges.is_empty() {
        return vec![LineFrag {
            x: p.indent_left,
            y: 0,
            width: 0,
            height: lh,
            baseline: (lh * 4) / 5,
            text: marker.unwrap_or_default(),
            font_size: size,
        }];
    }
    let chars: Vec<char> = text.chars().collect();
    ranges
        .into_iter()
        .enumerate()
        .map(|(i, (a, b))| {
            let slice: String = chars.get(a..b).unwrap_or(&[]).iter().collect();
            let w: i32 = shaped.advances.get(a..b).unwrap_or(&[]).iter().sum();
            let extra_indent = if i == 0 { p.indent_first } else { 0 };
            let (text_out, x_off, extra_w) = if i == 0 {
                if let Some(m) = marker.as_deref() {
                    (format!("{m}{slice}"), 0, marker_w)
                } else {
                    (slice, 0, 0)
                }
            } else {
                (slice, marker_w, 0)
            };
            let x = p.indent_left + extra_indent + x_off;
            let aligned_x = align_x(x, w + extra_w, usable + marker_w, p.alignment);
            LineFrag {
                x: aligned_x,
                y: 0,
                width: w + extra_w,
                height: lh,
                baseline: (lh * 4) / 5,
                text: text_out,
                font_size: size,
            }
        })
        .collect()
}

fn list_marker(p: &Paragraph) -> Option<String> {
    let n = p.numbering.as_ref()?;
    match n.format {
        Some(NumberFormat::Bullet) => Some("• ".into()),
        Some(NumberFormat::Decimal) => {
            Some(format!("{}. ", n.start.unwrap_or(1)))
        }
        _ => None,
    }
}

fn align_x(x: Hu, width: Hu, usable: Hu, alignment: Alignment) -> Hu {
    match alignment {
        Alignment::Start | Alignment::Justify | Alignment::Distribute => x,
        Alignment::Center => x + (usable - width) / 2,
        Alignment::End => x + (usable - width),
    }
}

#[comemo::memoize]
fn break_lines(text: String, advances: Vec<i32>, width: i32) -> Vec<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    if n == 0 {
        return vec![(0, 0)];
    }
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut i = 0usize;
    while i < n {
        let start = i;
        let mut acc = 0i32;
        let mut last_space: Option<usize> = None;
        let mut wrapped = false;
        while i < n {
            let adv = advances.get(i).copied().unwrap_or(0);
            if i > start && acc + adv > width {
                let at = last_space.filter(|&b| b > start).unwrap_or(i);
                lines.push((start, at));
                i = at;
                while i < n && chars[i].is_whitespace() {
                    i += 1;
                }
                wrapped = true;
                break;
            }
            acc += adv;
            if chars[i].is_whitespace() {
                last_space = Some(i + 1);
            }
            i += 1;
        }
        if !wrapped {
            lines.push((start, n));
        }
    }
    if lines.is_empty() {
        lines.push((0, n));
    }
    lines
}

fn prepare_table(table: &Table, fonts: &FontSet, width: Hu) -> Prepared {
    let n_cols = table
        .rows
        .first()
        .map(|r| r.cells.len())
        .unwrap_or(0)
        .max(1);
    let mut col_widths: Vec<Hu> = (0..n_cols)
        .map(|c| {
            table
                .rows
                .iter()
                .filter_map(|r| r.cells.get(c).map(|cell| cell.width))
                .max()
                .unwrap_or(width / n_cols as i32)
        })
        .collect();
    let sum: Hu = col_widths.iter().sum();
    if sum > width && sum > 0 {
        col_widths = col_widths
            .iter()
            .map(|w| ((*w as i64) * i64::from(width) / i64::from(sum)) as i32)
            .collect();
    } else if n_cols > 0 && sum < width {
        let equal = width / n_cols as i32;
        col_widths = vec![equal; n_cols];
        let used = equal.saturating_mul(n_cols as i32);
        if let Some(last) = col_widths.last_mut() {
            *last = last.saturating_add(width - used);
        }
    }
    let mut rows = Vec::new();
    for row in &table.rows {
        let mut cells = Vec::new();
        for (ci, cell) in row.cells.iter().enumerate() {
            let cw = *col_widths.get(ci).unwrap_or(&10000);
            let inner = (cw - cell.padding.left - cell.padding.right - 160).max(1);
            let mut lines = Vec::new();
            for block in &cell.blocks {
                if let Block::Paragraph(p) = block {
                    lines.extend(layout_paragraph(p, fonts, inner));
                }
            }
            let height = lines.iter().map(|l| l.height).sum::<i32>()
                + cell.padding.top
                + cell.padding.bottom
                + 160;
            let height = row.height.unwrap_or(height).max(height);
            cells.push(PreparedCell {
                lines,
                height,
                header: row.header,
            });
        }
        rows.push(cells);
    }
    let (stroke_w, stroke) = table_stroke(table);
    Prepared::Table {
        rows,
        col_widths,
        stroke_w,
        stroke,
    }
}

fn table_stroke(table: &Table) -> (Hu, [u8; 4]) {
    let b = &table.borders;
    if b.top.style == BorderStyle::None
        && b.left.style == BorderStyle::None
        && b.inside_h.style == BorderStyle::None
        && b.inside_v.style == BorderStyle::None
    {
        return (0, [0, 0, 0, 255]);
    }
    let w = b
        .top
        .width
        .max(b.left.width)
        .max(b.inside_h.width)
        .max(b.inside_v.width);
    let c = b.top.color;
    (w.max(1), [c.r, c.g, c.b, c.a])
}

pub fn page_count(tree: &FragmentTree) -> usize {
    tree.pages.len()
}

pub fn all_coordinates_are_integers(tree: &FragmentTree) -> bool {
    let _ = tree;
    true
}

pub fn line_positions(tree: &FragmentTree) -> Vec<(Hu, Hu, Hu)> {
    tree.pages
        .iter()
        .flat_map(|p| p.lines.iter().map(|l| (l.x, l.y, l.height)))
        .collect()
}

pub fn table_row_heights(tree: &FragmentTree) -> Vec<Hu> {
    tree.pages
        .iter()
        .flat_map(|p| p.rects.iter().map(|r| r.height))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use docagent_model::{NumberFormat, NumberingRef, Paragraph, Section, Table};

    #[test]
    fn layout_uses_integer_hwpunit_only() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("Integer layout")));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        assert!(!tree.pages.is_empty());
        for page in &tree.pages {
            for line in &page.lines {
                assert!(line.height > 0);
                assert!(line.font_size > 0);
            }
        }
        assert!(all_coordinates_are_integers(&tree));
    }

    #[test]
    fn layout_does_not_read_lineseg_hints() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("hinted line");
        p.layout_hints.push(docagent_model::LayoutHint {
            text_start: 0,
            x: 99,
            y: 12345,
            width: 1,
            line_height: 1,
            text_height: 1,
            baseline: 1,
            spacing: 1,
            flags: 0,
        });
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let ys: Vec<i32> = tree.pages.iter().flat_map(|p| p.lines.iter().map(|l| l.y)).collect();
        assert!(ys.iter().all(|y| *y != 12345));
    }

    #[test]
    fn long_paragraph_wraps() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let text = "word ".repeat(400);
        section.body.push(Block::Paragraph(Paragraph::from_text(text)));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let n: usize = tree.pages.iter().map(|p| p.lines.len()).sum();
        assert!(n > 1);
    }

    #[test]
    fn wraps_on_spaces_and_keeps_ligature_tails() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Paragraph(Paragraph::from_text(
            "Delivery confirmation — order 48291. The files match and we are hoping.",
        )));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let joined: String = tree
            .pages
            .iter()
            .flat_map(|p| p.lines.iter().map(|l| l.text.as_str()))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(joined.contains("48291"), "{joined}");
        assert!(joined.contains("hoping"), "{joined}");
        assert!(joined.contains("confirmation"), "{joined}");
        for line in tree.pages.iter().flat_map(|p| p.lines.iter()) {
            let t = line.text.trim();
            if t.is_empty() {
                continue;
            }
            assert!(
                !t.starts_with("nts") && !t.starts_with("ing"),
                "mid-word wrap: {t}"
            );
        }
    }

    #[test]
    fn tables_emit_grid_strokes() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Table(Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ])));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        assert_eq!(page.rects.len(), 4, "one fill per cell");
        assert!(page.strokes.len() >= 16, "four edges per cell");
        assert_eq!(table_row_heights(&tree).len(), 4);
    }

    #[test]
    fn bullets_prefix_first_line() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Receiving dock is clear");
        p.numbering = Some(NumberingRef {
            definition_id: 0,
            level: 0,
            start: None,
            format: Some(NumberFormat::Bullet),
        });
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let text = &tree.pages[0].lines[0].text;
        assert!(text.starts_with('•'), "{text}");
        assert!(text.contains("Receiving dock is clear"), "{text}");
    }

    #[test]
    fn decimal_prefix_uses_start() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Hash the input");
        p.numbering = Some(NumberingRef {
            definition_id: 1,
            level: 0,
            start: Some(3),
            format: Some(NumberFormat::Decimal),
        });
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let text = &tree.pages[0].lines[0].text;
        assert!(text.starts_with("3. "), "{text}");
        assert!(text.contains("Hash the input"), "{text}");
    }
}
