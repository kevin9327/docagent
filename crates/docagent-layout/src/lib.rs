//! Integer-coordinate layout. One engine. No I/O. No format identifiers.
//!
//! LineSeg hints on the IR are ignored here; the conformance crate is the
//! only consumer.

#![forbid(unsafe_code)]

use docagent_font::{line_height, shape, FontSet};
use docagent_model::{
    Alignment, Block, BorderStyle, BreakKind, Document, Float, HeaderFooter, Hu, Hu64,
    InlineObject, LineSpacing, NumberFormat, Paragraph, RunContent, Section, Table,
    DEFAULT_FONT_SIZE_HU,
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
    /// Table grid strokes and heading rules. Not counted by `table_row_heights`.
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
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub code: bool,
    pub highlight: bool,
    /// External URI for this run, if it is a hyperlink.
    pub href: Option<String>,
    /// Inline raster occupying `width` × stored height in HWPUNIT.
    pub image: Option<InlineImage>,
}

/// PNG/JPEG bytes laid out as an inline run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineImage {
    pub bytes: Vec<u8>,
    pub mime: String,
    pub width: Hu,
    pub height: Hu,
}

impl LineFrag {
    /// Hit box plus URI for a PDF `/Link` annotation. Height covers glyphs
    /// even when this fragment is not the last on the line (`height` is 0).
    pub fn link_hit(&self) -> Option<(Hu, Hu, Hu, Hu, &str)> {
        let uri = self.href.as_deref().filter(|u| !u.is_empty())?;
        if self.width <= 0 {
            return None;
        }
        let h = self
            .height
            .max(self.font_size)
            .max(self.baseline.saturating_add(120));
        Some((self.x, self.y, self.width, h, uri))
    }

    /// Painted image box in the same top-left HWPUNIT space as `x`/`y`.
    pub fn image_box(&self) -> Option<(Hu, Hu, Hu, Hu)> {
        let img = self.image.as_ref()?;
        if img.width <= 0 || img.height <= 0 {
            return None;
        }
        Some((self.x, self.y, img.width, img.height))
    }
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
            Prepared::Lines {
                lines,
                space_before,
                space_after,
                rule,
                fills,
            } => {
                y += space_before;
                let mut text_left: Option<Hu> = None;
                let mut text_right = origin_x;
                let mut block_bottom = y;
                let mut first_line_y = y;
                let mut saw_line = false;
                let mut i = 0;
                while let Some((a, b, row_h)) = next_line_row(&lines, i) {
                    i = b;
                    if y + row_h > origin_y + content_h && !current.lines.is_empty() {
                        pages.push(std::mem::replace(
                            &mut current,
                            empty_page(page.width, page.height),
                        ));
                        y = origin_y + space_before;
                    }
                    if !saw_line {
                        first_line_y = y;
                        saw_line = true;
                    }
                    for line in &lines[a..b] {
                        let placed = place_row_frag(line, origin_x, y);
                        if placed.width > 0 {
                            text_left = Some(text_left.map_or(placed.x, |l| l.min(placed.x)));
                            text_right = text_right.max(placed.x.saturating_add(placed.width));
                        }
                        push_underlined_line(&mut current, placed);
                    }
                    y += row_h;
                    if row_h > 0 {
                        block_bottom = y;
                    }
                }
                if let (Some(rule), Some(left)) = (rule, text_left) {
                    let (rx, rw) = if rule.span_content {
                        (origin_x, content_w)
                    } else {
                        (left, (text_right - left).max(1))
                    };
                    current.strokes.push(RectFrag {
                        x: rx,
                        y: block_bottom.saturating_add(80),
                        width: rw,
                        height: rule.thickness,
                        fill: rule.color,
                    });
                }
                for fill in fills {
                    current.strokes.push(RectFrag {
                        x: origin_x.saturating_add(fill.x),
                        y: first_line_y.saturating_add(fill.y),
                        width: fill.width,
                        height: fill.height,
                        fill: fill.fill,
                    });
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
                                [204, 251, 241, 255]
                            } else {
                                [255, 255, 255, 255]
                            },
                        });
                        if stroke_w > 0 {
                            push_cell_strokes(&mut current.strokes, x, y, cw, row_h, stroke_w, stroke);
                        }
                        let mut ly = y + 80;
                        let mut li = 0;
                        while let Some((a, b, row_h)) = next_line_row(&cell.lines, li) {
                            li = b;
                            for line in &cell.lines[a..b] {
                                let placed = place_row_frag(line, x + 80, ly);
                                push_underlined_line(&mut current, placed);
                            }
                            ly += row_h;
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
    // Stamp after body so pagination still sees a fresh page as empty.
    stamp_header_footer(section, fonts, content_w, origin_x, &mut pages);
    pages
}

fn prepare_header_footer(hf: &HeaderFooter, fonts: &FontSet, width: Hu) -> Vec<Prepared> {
    hf.blocks
        .par_iter()
        .map(|block| prepare_block(block, fonts, width))
        .collect()
}

fn stamp_header_footer(
    section: &Section,
    fonts: &FontSet,
    content_w: Hu,
    origin_x: Hu,
    pages: &mut [PageFrag],
) {
    let page = &section.page;
    let header = section
        .header
        .as_ref()
        .map(|hf| prepare_header_footer(hf, fonts, content_w));
    let footer = section
        .footer
        .as_ref()
        .map(|hf| prepare_header_footer(hf, fonts, content_w));
    if header.is_none() && footer.is_none() {
        return;
    }
    let header_y = page.margin_top.saturating_sub(page.header_distance).max(0);
    let footer_y = page.height.saturating_sub(page.margin_bottom).max(0);
    for frag in pages {
        if let Some(items) = header.as_deref() {
            place_prepared_band(frag, items, origin_x, header_y, content_w, page.height);
        }
        if let Some(items) = footer.as_deref() {
            place_prepared_band(frag, items, origin_x, footer_y, content_w, page.height);
        }
    }
}

fn place_prepared_band(
    page: &mut PageFrag,
    items: &[Prepared],
    origin_x: Hu,
    origin_y: Hu,
    content_w: Hu,
    page_h: Hu,
) {
    let mut y = origin_y.max(0);
    for item in items {
        match item {
            Prepared::Lines {
                lines,
                space_before,
                space_after,
                rule,
                fills,
            } => {
                y = y.saturating_add(*space_before);
                let mut text_left: Option<Hu> = None;
                let mut text_right = origin_x;
                let mut block_bottom = y;
                let mut first_line_y = y;
                let mut saw_line = false;
                let mut i = 0;
                while let Some((a, b, row_h)) = next_line_row(lines, i) {
                    i = b;
                    if y >= page_h {
                        break;
                    }
                    if !saw_line {
                        first_line_y = y;
                        saw_line = true;
                    }
                    for line in &lines[a..b] {
                        let placed = place_row_frag(line, origin_x, y);
                        if placed.width > 0 {
                            text_left = Some(text_left.map_or(placed.x, |l| l.min(placed.x)));
                            text_right = text_right.max(placed.x.saturating_add(placed.width));
                        }
                        push_underlined_line(page, placed);
                    }
                    y = y.saturating_add(row_h);
                    if row_h > 0 {
                        block_bottom = y;
                    }
                }
                if let (Some(rule), Some(left)) = (rule.as_ref(), text_left) {
                    let (rx, rw) = if rule.span_content {
                        (origin_x, content_w)
                    } else {
                        (left, (text_right - left).max(1))
                    };
                    page.strokes.push(RectFrag {
                        x: rx,
                        y: block_bottom.saturating_add(80),
                        width: rw,
                        height: rule.thickness,
                        fill: rule.color,
                    });
                }
                for fill in fills {
                    page.strokes.push(RectFrag {
                        x: origin_x.saturating_add(fill.x),
                        y: first_line_y.saturating_add(fill.y),
                        width: fill.width,
                        height: fill.height,
                        fill: fill.fill,
                    });
                }
                y = y.saturating_add(*space_after);
            }
            Prepared::Table {
                rows,
                col_widths,
                stroke_w,
                stroke,
            } => {
                let x0 = origin_x;
                for row in rows {
                    let row_h = row
                        .iter()
                        .map(|c| c.height)
                        .max()
                        .unwrap_or(0)
                        .max(MIN_ROW_REMAINDER_HU);
                    if y >= page_h {
                        break;
                    }
                    let mut x = x0;
                    for (ci, cell) in row.iter().enumerate() {
                        let cw = *col_widths.get(ci).unwrap_or(&10000);
                        page.rects.push(RectFrag {
                            x,
                            y,
                            width: cw,
                            height: row_h,
                            fill: if cell.header {
                                [204, 251, 241, 255]
                            } else {
                                [255, 255, 255, 255]
                            },
                        });
                        if *stroke_w > 0 {
                            push_cell_strokes(&mut page.strokes, x, y, cw, row_h, *stroke_w, *stroke);
                        }
                        let mut ly = y + 80;
                        let mut li = 0;
                        while let Some((a, b, row_h)) = next_line_row(&cell.lines, li) {
                            li = b;
                            for line in &cell.lines[a..b] {
                                let placed = place_row_frag(line, x + 80, ly);
                                push_underlined_line(page, placed);
                            }
                            ly = ly.saturating_add(row_h);
                        }
                        x += cw;
                    }
                    y = y.saturating_add(row_h);
                }
            }
        }
    }
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

/// Prepared rows store the line box on the last fragment (`height > 0`);
/// earlier fragments keep `height == 0` so they share `y`.
fn next_line_row(lines: &[LineFrag], start: usize) -> Option<(usize, usize, Hu)> {
    if start >= lines.len() {
        return None;
    }
    let mut end = start;
    let mut row_h: Hu = 0;
    while end < lines.len() {
        let line = &lines[end];
        let img_h = line.image.as_ref().map(|im| im.height).unwrap_or(0);
        row_h = row_h.max(line.height).max(img_h);
        end += 1;
        if line.height > 0 {
            break;
        }
    }
    Some((start, end, row_h))
}

fn place_row_frag(line: &LineFrag, origin_x: Hu, y: Hu) -> LineFrag {
    let mut placed = line.clone();
    placed.x = origin_x.saturating_add(placed.x);
    placed.y = y;
    if let Some(img) = &placed.image {
        placed.height = placed.height.max(img.height);
    }
    placed
}

fn push_underlined_line(page: &mut PageFrag, placed: LineFrag) {
    if placed.underline && placed.width > 0 {
        page.strokes.push(RectFrag {
            x: placed.x,
            y: placed.y.saturating_add(placed.baseline).saturating_add(40),
            width: placed.width,
            height: 80,
            fill: [19, 78, 74, 255],
        });
    }
    if placed.strike && placed.width > 0 {
        page.strokes.push(RectFrag {
            x: placed.x,
            y: placed
                .y
                .saturating_add(placed.baseline)
                .saturating_sub(placed.font_size / 3),
            width: placed.width,
            height: 80,
            fill: [19, 78, 74, 255],
        });
    }
    if placed.code && placed.width > 0 {
        let h = placed.font_size.saturating_add(80).max(1);
        page.strokes.push(RectFrag {
            x: placed.x,
            y: placed
                .y
                .saturating_add(placed.baseline)
                .saturating_sub(placed.font_size)
                .saturating_sub(40),
            width: placed.width,
            height: h,
            fill: [204, 251, 241, 255],
        });
    }
    if placed.highlight && placed.width > 0 {
        let h = placed.font_size.saturating_add(80).max(1);
        page.strokes.push(RectFrag {
            x: placed.x,
            y: placed
                .y
                .saturating_add(placed.baseline)
                .saturating_sub(placed.font_size)
                .saturating_sub(40),
            width: placed.width,
            height: h,
            fill: [253, 230, 138, 255],
        });
    }
    page.lines.push(placed);
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
        rule: Option<HeadingRule>,
        fills: Vec<RectFrag>,
    },
    Table {
        rows: Vec<Vec<PreparedCell>>,
        col_widths: Vec<Hu>,
        stroke_w: Hu,
        stroke: [u8; 4],
    },
}

struct HeadingRule {
    thickness: Hu,
    color: [u8; 4],
    span_content: bool,
}

fn heading_rule(level: Option<u8>) -> Option<HeadingRule> {
    match level {
        Some(1) => Some(HeadingRule {
            thickness: 200,
            color: [19, 78, 74, 255],
            span_content: true,
        }),
        Some(n) if n >= 2 => Some(HeadingRule {
            thickness: 100,
            color: [13, 148, 136, 255],
            span_content: false,
        }),
        _ => None,
    }
}

struct PreparedCell {
    lines: Vec<LineFrag>,
    height: Hu,
    header: bool,
}

fn prepare_block(block: &Block, fonts: &FontSet, width: Hu) -> Prepared {
    match block {
        Block::Paragraph(p) => {
            let (lines, fills) = layout_paragraph(p, fonts, width);
            Prepared::Lines {
                lines,
                space_before: p.space_before.max(0),
                space_after: p.space_after.max(0),
                rule: heading_rule(p.outline_level),
                fills,
            }
        }
        Block::Table(t) => prepare_table(t, fonts, width),
        Block::Break(BreakKind::Thematic) => Prepared::Lines {
            lines: Vec::new(),
            space_before: 240,
            space_after: 240,
            rule: None,
            fills: vec![RectFrag {
                x: 0,
                y: 0,
                width,
                height: 80,
                fill: [19, 78, 74, 255],
            }],
        },
        Block::Float(Float::Image(img)) => prepare_float_image(img, width),
        Block::Float(_) | Block::Break(_) => Prepared::Lines {
            lines: Vec::new(),
            space_before: 0,
            space_after: 0,
            rule: None,
            fills: Vec::new(),
        },
    }
}

fn prepare_float_image(img: &docagent_model::ImageData, max_w: Hu) -> Prepared {
    if img.width <= 0 || img.height <= 0 {
        return Prepared::Lines {
            lines: Vec::new(),
            space_before: 0,
            space_after: 0,
            rule: None,
            fills: Vec::new(),
        };
    }
    let mut image = InlineImage {
        bytes: img.bytes.clone(),
        mime: img.mime.clone(),
        width: img.width,
        height: img.height,
    };
    clamp_inline_image(&mut image, max_w);
    Prepared::Lines {
        lines: vec![LineFrag {
            x: 0,
            y: 0,
            width: image.width,
            height: image.height,
            baseline: image.height,
            text: String::new(),
            font_size: DEFAULT_FONT_SIZE_HU,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            code: false,
            highlight: false,
            href: None,
            image: Some(image),
        }],
        space_before: 0,
        space_after: 0,
        rule: None,
        fills: Vec::new(),
    }
}

fn is_bullet(p: &Paragraph) -> bool {
    matches!(
        p.numbering.as_ref().and_then(|n| n.format),
        Some(NumberFormat::Bullet)
    )
}

fn is_task(p: &Paragraph) -> bool {
    matches!(
        p.numbering.as_ref().and_then(|n| n.format),
        Some(NumberFormat::Task)
    )
}

fn task_checked(p: &Paragraph) -> bool {
    p.numbering.as_ref().is_some_and(|n| n.start == Some(1))
}

fn bullet_disc(size: Hu) -> Hu {
    // ~0.75em so PDF/A markers read as discs/boxes, not 2pt dots.
    (size.saturating_mul(3) / 4).max(400)
}

fn marker_advance(size: Hu, glyph_w: Hu, box_mark: bool) -> Hu {
    if box_mark {
        bullet_disc(size).saturating_add(240).max(glyph_w)
    } else {
        glyph_w
    }
}

fn layout_paragraph(p: &Paragraph, fonts: &FontSet, width: Hu) -> (Vec<LineFrag>, Vec<RectFrag>) {
    let (text, mut spans) = collect_runs(p);
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
    let baseline = (lh * 4) / 5;
    let marker = list_marker(p);
    let glyph_w = marker
        .as_deref()
        .map(|m| shape(fonts, m, size).width)
        .unwrap_or(0);
    let quote_pad = if p.quote {
        720
    } else if p.code_block {
        200
    } else {
        0
    };
    let bullet = is_bullet(p);
    let task = is_task(p);
    let box_mark = bullet || task;
    let marker_w = marker_advance(size, glyph_w, box_mark);
    let usable = (width - p.indent_left - quote_pad - p.indent_right - marker_w).max(1);
    for span in &mut spans {
        if let Some(img) = span.image.as_mut() {
            clamp_inline_image(img, usable);
        }
    }
    let mut shaped = shape(fonts, &text, size);
    for span in &spans {
        if let Some(img) = &span.image
            && let Some(adv) = shaped.advances.get_mut(span.start)
        {
            *adv = img.width.max(1);
        }
    }
    let ranges = break_lines(text.clone(), shaped.advances.clone(), usable);
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut fills = Vec::new();
    if ranges.is_empty() {
        out.push(LineFrag {
            x: p.indent_left + quote_pad,
            y: 0,
            width: 0,
            height: lh,
            baseline,
            text: if box_mark {
                String::new()
            } else {
                marker.unwrap_or_default()
            },
            font_size: size,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            code: false,
            highlight: false,
            href: None,
            image: None,
        });
        if bullet {
            fills.push(bullet_fill(p.indent_left + quote_pad, baseline, size));
        }
        if task {
            fills.extend(task_fills(
                p.indent_left + quote_pad,
                baseline,
                size,
                task_checked(p),
            ));
        }
        if p.quote {
            fills.push(RectFrag {
                x: p.indent_left,
                y: 0,
                width: 80,
                height: lh,
                fill: [19, 78, 74, 255],
            });
        }
        if p.code_block {
            fills.push(RectFrag {
                x: p.indent_left,
                y: 0,
                width,
                height: lh,
                fill: [204, 251, 241, 255],
            });
        }
        return (out, fills);
    }
    for (li, (a, b)) in ranges.into_iter().enumerate() {
        let extra_indent = if li == 0 { p.indent_first } else { 0 };
        let mut x = p.indent_left + quote_pad + extra_indent + if li == 0 { 0 } else { marker_w };
        let row_right = p.indent_left + quote_pad + extra_indent + usable + marker_w;
        let mut row: Vec<LineFrag> = Vec::new();
        if li == 0 && bullet {
            fills.push(bullet_fill(x, baseline, size));
            x += marker_w;
        } else if li == 0 && task {
            fills.extend(task_fills(x, baseline, size, task_checked(p)));
            x += marker_w;
        } else if li == 0
            && let Some(m) = marker.as_deref()
        {
            row.push(LineFrag {
                x,
                y: 0,
                width: marker_w,
                height: 0,
                baseline,
                text: m.to_string(),
                font_size: size,
                bold: false,
                italic: false,
                underline: false,
                strike: false,
                code: false,
                highlight: false,
                href: None,
                image: None,
            });
            x += marker_w;
        }
        for span in &spans {
            let s = span.start.max(a);
            let e = span.end.min(b);
            if s >= e {
                continue;
            }
            if let Some(img) = &span.image {
                let mut img = img.clone();
                clamp_inline_image(&mut img, (row_right - x).max(1));
                let w = img.width;
                row.push(LineFrag {
                    x,
                    y: 0,
                    width: w,
                    height: 0,
                    baseline,
                    text: String::new(),
                    font_size: size,
                    bold: span.bold,
                    italic: span.italic,
                    underline: span.underline,
                    strike: span.strike,
                    code: span.code,
                    highlight: span.highlight,
                    href: span.href.clone(),
                    image: Some(img),
                });
                x += w;
                continue;
            }
            let slice: String = chars.get(s..e).unwrap_or(&[]).iter().collect();
            let (w, run_size, run_baseline) = if span.superscript {
                let rs = (size.saturating_mul(2) / 3).max(1);
                (
                    shape(fonts, &slice, rs).width,
                    rs,
                    baseline.saturating_sub(size / 3),
                )
            } else if span.subscript {
                let rs = (size.saturating_mul(2) / 3).max(1);
                (
                    shape(fonts, &slice, rs).width,
                    rs,
                    baseline.saturating_add(size / 5),
                )
            } else {
                let w: i32 = shaped.advances.get(s..e).unwrap_or(&[]).iter().sum();
                (w, size, baseline)
            };
            row.push(LineFrag {
                x,
                y: 0,
                width: w,
                height: 0,
                baseline: run_baseline,
                text: slice,
                font_size: run_size,
                bold: span.bold,
                italic: span.italic,
                underline: span.underline,
                strike: span.strike,
                code: span.code,
                highlight: span.highlight,
                href: span.href.clone(),
                image: None,
            });
            x += w;
        }
        let total_w = row.iter().map(|f| f.width).sum::<i32>();
        let origin = p.indent_left + quote_pad + extra_indent;
        let shift = align_x(origin, total_w, usable + marker_w, p.alignment) - origin;
        for f in &mut row {
            f.x += shift;
        }
        let mut row_h = lh;
        for f in &row {
            if let Some(img) = &f.image {
                row_h = row_h.max(img.height);
            }
        }
        if let Some(last) = row.last_mut() {
            last.height = row_h;
        } else {
            row.push(LineFrag {
                x: p.indent_left + quote_pad,
                y: 0,
                width: 0,
                height: lh,
                baseline,
                text: String::new(),
                font_size: size,
                bold: false,
                italic: false,
                underline: false,
                strike: false,
                code: false,
                highlight: false,
                href: None,
                image: None,
            });
        }
        out.extend(row);
    }
    if p.quote {
        let h = out.iter().map(|l| l.height).sum::<i32>().max(lh);
        fills.push(RectFrag {
            x: p.indent_left,
            y: 0,
            width: 80,
            height: h,
            fill: [19, 78, 74, 255],
        });
    }
    if p.code_block {
        let h = out.iter().map(|l| l.height).sum::<i32>().max(lh);
        fills.push(RectFrag {
            x: p.indent_left,
            y: 0,
            width,
            height: h,
            fill: [204, 251, 241, 255],
        });
    }
    (out, fills)
}

fn bullet_fill(x: Hu, baseline: Hu, size: Hu) -> RectFrag {
    let disc = bullet_disc(size);
    RectFrag {
        x: x.saturating_add(80),
        y: baseline.saturating_sub(disc),
        width: disc,
        height: disc,
        fill: [19, 78, 74, 255],
    }
}

fn task_fills(x: Hu, baseline: Hu, size: Hu, checked: bool) -> Vec<RectFrag> {
    let s = bullet_disc(size);
    let x = x.saturating_add(80);
    let y = baseline.saturating_sub(s);
    let t = (s / 6).max(40);
    let teal = [19, 78, 74, 255];
    let mut out = vec![
        RectFrag {
            x,
            y,
            width: s,
            height: t,
            fill: teal,
        },
        RectFrag {
            x,
            y,
            width: t,
            height: s,
            fill: teal,
        },
        RectFrag {
            x,
            y: y + s - t,
            width: s,
            height: t,
            fill: teal,
        },
        RectFrag {
            x: x + s - t,
            y,
            width: t,
            height: s,
            fill: teal,
        },
    ];
    if checked {
        let inset = t;
        let inner = (s - inset.saturating_mul(2)).max(1);
        out.push(RectFrag {
            x: x + inset,
            y: y + inset,
            width: inner,
            height: inner,
            fill: teal,
        });
    }
    out
}

struct RunSpan {
    start: usize,
    end: usize,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    code: bool,
    highlight: bool,
    superscript: bool,
    subscript: bool,
    href: Option<String>,
    image: Option<InlineImage>,
}

fn clamp_inline_image(img: &mut InlineImage, max_w: Hu) {
    if img.width <= max_w {
        return;
    }
    let w = max_w.max(1);
    let h = (Hu64::from(img.height) * Hu64::from(w) / Hu64::from(img.width.max(1))).max(1) as Hu;
    img.width = w;
    img.height = h;
}

fn collect_runs(p: &Paragraph) -> (String, Vec<RunSpan>) {
    let mut i = 0usize;
    let mut text = String::new();
    let mut out = Vec::new();
    for run in &p.runs {
        let href = match &run.content {
            RunContent::Inline(InlineObject::Hyperlink { target, .. }) if !target.is_empty() => {
                Some(target.clone())
            }
            _ => None,
        };
        let underline = run.style.underline != docagent_model::Underline::None || href.is_some();
        if let RunContent::Inline(InlineObject::Image(img)) = &run.content {
            if img.width <= 0 || img.height <= 0 {
                continue;
            }
            text.push('\u{FFFC}');
            out.push(RunSpan {
                start: i,
                end: i + 1,
                bold: run.style.bold,
                italic: run.style.italic,
                underline,
                strike: run.style.strike,
                code: run.style.code,
                highlight: run.style.highlight.is_some(),
                superscript: run.style.superscript,
                subscript: run.style.subscript,
                href,
                image: Some(InlineImage {
                    bytes: img.bytes.clone(),
                    mime: img.mime.clone(),
                    width: img.width,
                    height: img.height,
                }),
            });
            i += 1;
            continue;
        }
        let t = run.display_text();
        if t.is_empty() {
            continue;
        }
        let n = t.chars().count();
        text.push_str(t);
        out.push(RunSpan {
            start: i,
            end: i + n,
            bold: run.style.bold,
            italic: run.style.italic,
            underline,
            strike: run.style.strike,
            code: run.style.code,
            highlight: run.style.highlight.is_some(),
            superscript: run.style.superscript,
            subscript: run.style.subscript,
            href,
            image: None,
        });
        i += n;
    }
    (text, out)
}

fn list_marker(p: &Paragraph) -> Option<String> {
    let n = p.numbering.as_ref()?;
    match n.format {
        Some(NumberFormat::Bullet) | Some(NumberFormat::Task) => Some("• ".into()),
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
    for (ri, row) in table.rows.iter().enumerate() {
        let mut cells = Vec::new();
        let header = row.header || ri < table.header_row_count as usize;
        for (ci, cell) in row.cells.iter().enumerate() {
            let cw = *col_widths.get(ci).unwrap_or(&10000);
            let inner = (cw - cell.padding.left - cell.padding.right - 160).max(1);
            let mut lines = Vec::new();
            for block in &cell.blocks {
                if let Block::Paragraph(p) = block {
                    let (cell_lines, _) = layout_paragraph(p, fonts, inner);
                    lines.extend(cell_lines);
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
                header,
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
    use docagent_model::{
        HeaderFooter, NumberFormat, NumberingRef, Paragraph, Section, Table, DEFAULT_MARGIN_HU,
    };

    fn hf(text: &str) -> HeaderFooter {
        HeaderFooter {
            blocks: vec![Block::Paragraph(Paragraph::from_text(text))],
            different_first: None,
            different_odd_even: None,
        }
    }

    fn hf_image(width: Hu, height: Hu) -> HeaderFooter {
        let mut p = Paragraph::from_text("");
        p.runs = vec![mark_run(width, height)];
        HeaderFooter {
            blocks: vec![Block::Paragraph(p)],
            different_first: None,
            different_odd_even: None,
        }
    }

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
    fn table_headers_emit_teal_fills() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let headers = page
            .rects
            .iter()
            .filter(|r| r.fill == [204, 251, 241, 255])
            .count();
        let body = page
            .rects
            .iter()
            .filter(|r| r.fill == [255, 255, 255, 255])
            .count();
        assert_eq!(headers, 2, "header cells missing teal fill: {:?}", page.rects);
        assert_eq!(body, 2, "body cells missing white fill: {:?}", page.rects);
    }

    #[test]
    fn headings_emit_rule_fills() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.size = 1800;
        h1.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation");
        h2.outline_level = Some(2);
        h2.runs[0].style.size = 1400;
        h2.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h2));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("Body copy")));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        assert!(
            page.rects.is_empty(),
            "heading rules must not count as table cell fills"
        );
        assert!(
            page.strokes.iter().any(|s| s.fill == [19, 78, 74, 255] && s.height == 200 && s.width > 1000),
            "h1 rule missing: {:?}",
            page.strokes
        );
        assert!(
            page.strokes.iter().any(|s| s.fill == [13, 148, 136, 255] && s.height == 100 && s.width > 0),
            "h2 rule missing: {:?}",
            page.strokes
        );
        assert_eq!(table_row_heights(&tree).len(), 0);
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
        let joined: String = tree.pages[0]
            .lines
            .iter()
            .map(|l| l.text.as_str())
            .collect();
        assert!(!joined.contains('•'), "{joined}");
        assert!(joined.contains("Receiving dock is clear"), "{joined}");
        assert!(
            tree.pages[0]
                .strokes
                .iter()
                .any(|s| s.fill == [19, 78, 74, 255] && s.width == s.height && s.width >= 400),
            "bullet marker fill missing: {:?}",
            tree.pages[0].strokes
        );
    }

    #[test]
    fn task_boxes_emit_checkbox_fills() {
        fn tree_for(checked: bool) -> FragmentTree {
            let mut doc = Document::new();
            let mut section = Section::default();
            let mut p = Paragraph::from_text("Replay the convert");
            p.numbering = Some(NumberingRef {
                definition_id: 2,
                level: 0,
                start: checked.then_some(1),
                format: Some(NumberFormat::Task),
            });
            section.body.push(Block::Paragraph(p));
            doc.sections.push(section);
            layout_document(&doc, &FontSet::bundled())
        }
        let open = tree_for(false);
        let edges = open.pages[0]
            .strokes
            .iter()
            .filter(|s| s.fill == [19, 78, 74, 255] && s.width != s.height)
            .count();
        assert!(edges >= 4, "unchecked box missing edges: {:?}", open.pages[0].strokes);
        let done = tree_for(true);
        let inner = done.pages[0].strokes.iter().any(|s| {
            s.fill == [19, 78, 74, 255] && s.width == s.height && s.width >= 200
        });
        assert!(inner, "checked inner fill missing: {:?}", done.pages[0].strokes);
        let joined: String = done.pages[0]
            .lines
            .iter()
            .map(|l| l.text.as_str())
            .collect();
        assert!(!joined.contains('•'), "{joined}");
        assert!(joined.contains("Replay the convert"), "{joined}");
    }

    #[test]
    fn superscripts_shrink_and_raise() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("PDF/");
        let mut sup = docagent_model::Run::text("A");
        sup.style.superscript = true;
        p.runs.push(sup);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let base = page
            .lines
            .iter()
            .find(|l| l.text.contains("PDF"))
            .expect("base");
        let raised = page
            .lines
            .iter()
            .find(|l| l.text == "A")
            .expect("super");
        assert!(raised.font_size < base.font_size, "super size {} vs {}", raised.font_size, base.font_size);
        assert!(
            raised.y + raised.baseline < base.y + base.baseline,
            "super baseline {} vs {}",
            raised.y + raised.baseline,
            base.y + base.baseline
        );
    }

    #[test]
    fn subscripts_shrink_and_lower() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("H");
        let mut sub = docagent_model::Run::text("2");
        sub.style.subscript = true;
        p.runs.push(sub);
        p.runs.push(docagent_model::Run::text("O"));
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let base = page
            .lines
            .iter()
            .find(|l| l.text == "H")
            .expect("base");
        let lowered = page
            .lines
            .iter()
            .find(|l| l.text == "2")
            .expect("sub");
        assert!(lowered.font_size < base.font_size, "sub size {} vs {}", lowered.font_size, base.font_size);
        assert!(
            lowered.y + lowered.baseline > base.y + base.baseline,
            "sub baseline {} vs {}",
            lowered.y + lowered.baseline,
            base.y + base.baseline
        );
    }

    #[test]
    fn underlines_emit_fills() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("09:00 local");
        p.runs[0].style.underline = docagent_model::Underline::Single;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let line = page
            .lines
            .iter()
            .find(|l| l.text.contains("09:00"))
            .expect("underlined run");
        assert!(line.underline);
        assert!(line.href.is_none());
        assert!(
            page.strokes
                .iter()
                .any(|s| s.fill == [19, 78, 74, 255] && s.height == 80 && s.width > 80),
            "underline fill missing: {:?}",
            page.strokes
        );
    }

    #[test]
    fn hyperlinks_emit_underline_fills() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![docagent_model::Run::hyperlink(
            "DocAgent",
            "https://github.com/kevin9327/docagent",
        )];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        assert!(page.lines.iter().any(|l| l.text.contains("DocAgent") && l.underline));
        assert!(
            page.lines.iter().any(|l| {
                l.href.as_deref() == Some("https://github.com/kevin9327/docagent")
                    && l.link_hit().is_some()
            }),
            "hyperlink href missing: {:?}",
            page.lines
        );
        assert!(
            page.strokes
                .iter()
                .any(|s| s.fill == [19, 78, 74, 255] && s.height == 80 && s.width > 80),
            "hyperlink underline missing: {:?}",
            page.strokes
        );
    }

    #[test]
    fn highlights_emit_background_fills() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("three ");
        let mut mark = docagent_model::Run::text("hashes");
        mark.style.highlight = Some(docagent_model::Color::MARK);
        p.runs.push(mark);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let line = page
            .lines
            .iter()
            .find(|l| l.text.contains("hashes"))
            .expect("mark run");
        assert!(line.highlight);
        assert!(
            page.strokes.iter().any(|s| {
                s.fill == [253, 230, 138, 255] && s.width > 80 && s.height > 80
            }),
            "highlight fill missing: {:?}",
            page.strokes
        );
    }

    #[test]
    fn code_spans_emit_background_fills() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Run ");
        let mut code = docagent_model::Run::text("prove");
        code.style.code = true;
        p.runs.push(code);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let line = page
            .lines
            .iter()
            .find(|l| l.text.contains("prove"))
            .expect("code run");
        assert!(line.code);
        assert!(
            page.lines
                .iter()
                .any(|l| l.text.contains("Run") && !l.code)
        );
        assert!(
            page.strokes.iter().any(|s| {
                s.fill == [204, 251, 241, 255] && s.width > 80 && s.height > 80
            }),
            "code background missing: {:?}",
            page.strokes
        );
    }

    #[test]
    fn code_blocks_emit_background_fills() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("docagent prove examples/letter.md");
        p.code_block = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        assert!(
            page.lines
                .iter()
                .any(|l| l.text.contains("docagent prove"))
        );
        assert!(
            page.strokes.iter().any(|s| {
                s.fill == [204, 251, 241, 255] && s.width > 10000 && s.height > 80
            }),
            "code block fill missing: {:?}",
            page.strokes
        );
    }

    #[test]
    fn thematic_breaks_emit_rule_fills() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("before")));
        section.body.push(Block::Break(BreakKind::Thematic));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("after")));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        assert!(page.lines.iter().any(|l| l.text.contains("before")));
        assert!(page.lines.iter().any(|l| l.text.contains("after")));
        assert!(
            page.strokes.iter().any(|s| {
                s.fill == [19, 78, 74, 255] && s.height == 80 && s.width > 10000
            }),
            "thematic rule missing: {:?}",
            page.strokes
        );
    }

    #[test]
    fn quotes_emit_left_bar_fills() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        p.quote = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        assert!(
            page.lines
                .iter()
                .any(|l| l.text.contains("Replay is evidence"))
        );
        assert!(
            page.strokes.iter().any(|s| {
                s.fill == [19, 78, 74, 255] && s.width == 80 && s.height > 200
            }),
            "quote bar missing: {:?}",
            page.strokes
        );
    }

    #[test]
    fn strikethrough_emits_midline_fills() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("plain ");
        let mut strike = docagent_model::Run::text("guess");
        strike.style.strike = true;
        p.runs.push(strike);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let line = page
            .lines
            .iter()
            .find(|l| l.text.contains("guess"))
            .expect("struck run");
        assert!(line.strike);
        assert!(
            page.lines
                .iter()
                .any(|l| l.text.contains("plain") && !l.strike)
        );
        assert!(
            page.strokes.iter().any(|s| {
                s.fill == [19, 78, 74, 255]
                    && s.height == 80
                    && s.width > 80
                    && s.y < line.y.saturating_add(line.baseline)
            }),
            "strikethrough fill missing: {:?}",
            page.strokes
        );
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
        let joined: String = tree.pages[0]
            .lines
            .iter()
            .map(|l| l.text.as_str())
            .collect();
        assert!(joined.starts_with("3. "), "{joined}");
        assert!(joined.contains("Hash the input"), "{joined}");
    }

    #[test]
    fn nested_bullet_hangs_further() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut a = Paragraph::from_text("dock");
        a.indent_left = 1440;
        a.numbering = Some(NumberingRef {
            definition_id: 0,
            level: 0,
            start: None,
            format: Some(NumberFormat::Bullet),
        });
        let mut b = Paragraph::from_text("bay 4");
        b.indent_left = 2880;
        b.numbering = Some(NumberingRef {
            definition_id: 0,
            level: 1,
            start: None,
            format: Some(NumberFormat::Bullet),
        });
        section.body.push(Block::Paragraph(a));
        section.body.push(Block::Paragraph(b));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let parent = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.text.contains("dock"))
            .map(|l| l.x)
            .unwrap();
        let nested = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.text.contains("bay"))
            .map(|l| l.x)
            .unwrap();
        assert!(nested > parent, "parent={parent} nested={nested}");
    }

    #[test]
    fn bold_runs_mark_line_frags() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("plain ");
        let mut bold = docagent_model::Run::text("GPU-free");
        bold.style.bold = true;
        p.runs.push(bold);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let bold_frag = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.text.contains("GPU-free"))
            .expect("bold span");
        assert!(bold_frag.bold);
        let plain = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.text.contains("plain"))
            .expect("plain span");
        assert!(!plain.bold);
    }

    #[test]
    fn italic_runs_mark_line_frags() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("plain ");
        let mut italic = docagent_model::Run::text("byte-for-byte");
        italic.style.italic = true;
        p.runs.push(italic);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let italic_frag = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.text.contains("byte-for-byte"))
            .expect("italic span");
        assert!(italic_frag.italic);
        let plain = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.text.contains("plain"))
            .expect("plain span");
        assert!(!plain.italic);
    }

    const MARK_PNG: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/assets/mark.png"
    ));

    fn mark_run(width: Hu, height: Hu) -> docagent_model::Run {
        docagent_model::Run {
            style: docagent_model::CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(docagent_model::ImageData {
                bytes: MARK_PNG.to_vec(),
                mime: "image/png".into(),
                width,
                height,
                alt_text: Some("mark".into()),
                wrap: docagent_model::WrapMode::Inline,
            })),
        }
    }

    #[test]
    fn image_runs_reserve_width_and_height() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![mark_run(1600, 1600)];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let frag = page
            .lines
            .iter()
            .find(|l| l.image.is_some())
            .expect("image run skipped");
        assert_eq!(frag.width, 1600);
        assert!(frag.height >= 1600, "line height {}", frag.height);
        let img = frag.image.as_ref().expect("payload");
        assert_eq!(img.width, 1600);
        assert_eq!(img.height, 1600);
        assert_eq!(img.mime, "image/png");
        assert_eq!(img.bytes, MARK_PNG);
        assert!(frag.text.is_empty());
    }

    #[test]
    fn image_run_advances_following_text() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![
            docagent_model::Run::text("before"),
            mark_run(1600, 800),
            docagent_model::Run::text("after"),
        ];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let before = page
            .lines
            .iter()
            .find(|l| l.text.contains("before"))
            .expect("before");
        let img = page
            .lines
            .iter()
            .find(|l| l.image.is_some())
            .expect("image");
        let after = page
            .lines
            .iter()
            .find(|l| l.text.contains("after"))
            .expect("after");
        assert!(img.x >= before.x + before.width, "image x {}", img.x);
        assert!(after.x >= img.x + img.width, "after x {}", after.x);
        assert_eq!(img.width, 1600);
        assert_eq!(img.image.as_ref().map(|i| i.height), Some(800));
        assert_eq!(before.y, img.y);
        assert_eq!(after.y, img.y);
        assert!(img.height >= 800, "line height {}", img.height);
    }

    fn boxes_overlap(a: (Hu, Hu, Hu, Hu), b: (Hu, Hu, Hu, Hu)) -> bool {
        let (ax, ay, aw, ah) = a;
        let (bx, by, bw, bh) = b;
        aw > 0
            && ah > 0
            && bw > 0
            && bh > 0
            && ax < bx.saturating_add(bw)
            && ax.saturating_add(aw) > bx
            && ay < by.saturating_add(bh)
            && ay.saturating_add(ah) > by
    }

    fn letter_with_mark(width: Hu, height: Hu) -> Document {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut heading = Paragraph::from_text("Northwind Freight");
        heading.outline_level = Some(1);
        heading.runs[0].style.size = 1800;
        heading.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(heading));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![mark_run(width, height)];
        section.body.push(Block::Paragraph(img_p));
        section.body.push(Block::Paragraph(Paragraph::from_text(
            "The shipment left Busan on 4 September",
        )));
        doc.sections.push(section);
        doc
    }

    #[test]
    fn letter_inline_image_keeps_imagedata_size_without_overlapping_body() {
        const W: Hu = 2400;
        const H: Hu = 1600;
        let tree = layout_document(&letter_with_mark(W, H), &FontSet::bundled());
        let page = &tree.pages[0];
        let img_line = page
            .lines
            .iter()
            .find(|l| l.image.is_some())
            .expect("letter image dropped");
        let img = img_line.image.as_ref().expect("payload");
        assert_eq!(img.width, W);
        assert_eq!(img.height, H);
        assert_eq!(img.bytes, MARK_PNG);
        assert_eq!(img.mime, "image/png");
        assert_eq!(img_line.width, W);
        assert!(
            img_line.height >= H,
            "line box {} shorter than ImageData height {H}",
            img_line.height
        );
        let img_box = img_line.image_box().expect("image box");
        assert_eq!(img_box, (img_line.x, img_line.y, W, H));
        let body = page
            .lines
            .iter()
            .find(|l| l.text.contains("shipment"))
            .expect("body dropped");
        assert!(
            body.y >= img_line.y.saturating_add(H),
            "body y {} overlaps image y {} height {H}",
            body.y,
            img_line.y
        );
        let body_box = (body.x, body.y, body.width.max(1), body.height.max(1));
        assert!(
            !boxes_overlap(img_box, body_box),
            "image {img_box:?} overlaps body {body_box:?}"
        );
        let heading = page
            .lines
            .iter()
            .find(|l| l.text.contains("Northwind"))
            .expect("heading dropped");
        let heading_box = (
            heading.x,
            heading.y,
            heading.width.max(1),
            heading.height.max(1),
        );
        assert!(
            !boxes_overlap(img_box, heading_box),
            "image {img_box:?} overlaps heading {heading_box:?}"
        );
    }

    #[test]
    fn oversized_image_clamps_to_content_width() {
        let mut doc = Document::new();
        let mut section = Section::default();
        // US Letter: 8.5in × 11in at 7200 HU/in.
        section.page.width = 61_200;
        section.page.height = 79_200;
        let content_w = section.page.content_width();
        let mut p = Paragraph::from_text("");
        p.runs = vec![mark_run(50_000, 10_000)];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let frag = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.image.is_some())
            .expect("image run skipped");
        assert!(
            frag.width <= content_w,
            "frag.width {} exceeds content width {}",
            frag.width,
            content_w
        );
        let img = frag.image.as_ref().expect("payload");
        assert_eq!(img.width, frag.width);
        assert!(img.width <= content_w);
        assert_eq!(
            img.height,
            (i64::from(10_000) * i64::from(img.width) / 50_000) as i32
        );
        assert!(img.height > 0);
    }

    fn mark_image(width: Hu, height: Hu) -> docagent_model::ImageData {
        docagent_model::ImageData {
            bytes: MARK_PNG.to_vec(),
            mime: "image/png".into(),
            width,
            height,
            alt_text: Some("mark".into()),
            wrap: docagent_model::WrapMode::Square,
        }
    }

    #[test]
    fn float_image_yields_line_with_payload() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Float(Float::Image(mark_image(1600, 1600))));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let frag = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.image.is_some())
            .expect("float image dropped");
        assert_eq!(frag.width, 1600);
        assert!(frag.height >= 1600, "line height {}", frag.height);
        let img = frag.image.as_ref().expect("payload");
        assert_eq!(img.bytes, MARK_PNG);
        assert_eq!(img.mime, "image/png");
        assert_eq!(img.width, 1600);
        assert_eq!(img.height, 1600);
        assert!(frag.text.is_empty());
    }

    #[test]
    fn oversized_float_image_clamps_to_content_width() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let content_w = section.page.content_width();
        section
            .body
            .push(Block::Float(Float::Image(mark_image(50_000, 10_000))));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let frag = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.image.is_some())
            .expect("float image dropped");
        assert!(
            frag.width <= content_w,
            "frag.width {} exceeds content width {}",
            frag.width,
            content_w
        );
        let img = frag.image.as_ref().expect("payload");
        assert_eq!(img.width, frag.width);
        assert!(img.width <= content_w);
        assert_eq!(
            img.height,
            (i64::from(10_000) * i64::from(img.width) / 50_000) as i32
        );
        assert!(img.height > 0);
    }

    fn table_with_image_cell(width: Hu, height: Hu) -> Table {
        let mut table = Table::from_cells(vec![vec![String::new()]]);
        let mut p = Paragraph::from_text("");
        p.runs = vec![mark_run(width, height)];
        table.rows[0].cells[0].blocks = vec![Block::Paragraph(p)];
        table
    }

    #[test]
    fn table_cell_image_yields_line_with_payload() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Table(table_with_image_cell(1600, 1600)));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let frag = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.image.is_some())
            .expect("cell image dropped");
        assert_eq!(frag.width, 1600);
        assert!(frag.height >= 1600, "line height {}", frag.height);
        let img = frag.image.as_ref().expect("payload");
        assert_eq!(img.bytes, MARK_PNG);
        assert_eq!(img.mime, "image/png");
        assert_eq!(img.width, 1600);
        assert_eq!(img.height, 1600);
        assert!(frag.text.is_empty());
    }

    #[test]
    fn table_cell_image_clamps_to_cell_content_width() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![vec![String::new(), String::new()]]);
        table.rows[0].cells[0].width = 8_000;
        table.rows[0].cells[1].width = 8_000;
        let mut p = Paragraph::from_text("");
        p.runs = vec![mark_run(50_000, 10_000)];
        table.rows[0].cells[0].blocks = vec![Block::Paragraph(p)];
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let cell_w = page.rects.first().map(|r| r.width).expect("cell fill");
        let inner = (cell_w - 160).max(1);
        let frag = page
            .lines
            .iter()
            .find(|l| l.image.is_some())
            .expect("cell image dropped");
        assert!(
            frag.width <= inner,
            "frag.width {} exceeds cell content width {}",
            frag.width,
            inner
        );
        let img = frag.image.as_ref().expect("payload");
        assert_eq!(img.width, frag.width);
        assert!(img.width <= inner);
        assert_eq!(
            img.height,
            (i64::from(10_000) * i64::from(img.width) / 50_000) as i32
        );
        assert!(img.height > 0);
        assert_eq!(img.bytes, MARK_PNG);
    }

    #[test]
    fn section_header_appears_on_each_page() {
        let mut doc = Document::new();
        let mut section = Section {
            header: Some(hf("HEAD")),
            ..Section::default()
        };
        for _ in 0..80 {
            section
                .body
                .push(Block::Paragraph(Paragraph::from_text("body line")));
        }
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        assert!(
            tree.pages.len() >= 2,
            "need a second page, got {}",
            tree.pages.len()
        );
        for (i, page) in tree.pages.iter().enumerate() {
            let head = page
                .lines
                .iter()
                .find(|l| l.text.contains("HEAD"))
                .unwrap_or_else(|| panic!("HEAD missing on page {i}"));
            assert!(head.y >= 0, "header y {} off page", head.y);
            assert!(head.y < page.height, "header y {} past page", head.y);
            assert!(
                head.y < DEFAULT_MARGIN_HU,
                "header y {} not in top margin",
                head.y
            );
            assert!(
                page.lines.iter().any(|l| l.text.contains("body line")),
                "body dropped on page {i}"
            );
        }
    }

    #[test]
    fn section_footer_appears_on_page() {
        let mut doc = Document::new();
        let mut section = Section {
            footer: Some(hf("FOOT")),
            ..Section::default()
        };
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("body line")));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let foot = page
            .lines
            .iter()
            .find(|l| l.text.contains("FOOT"))
            .expect("FOOT missing");
        assert!(foot.y >= 0);
        assert!(foot.y < page.height);
        assert!(
            foot.y >= page.height.saturating_sub(DEFAULT_MARGIN_HU),
            "footer y {} not in bottom margin",
            foot.y
        );
        assert!(
            page.lines.iter().any(|l| l.text.contains("body line")),
            "body dropped"
        );
    }

    #[test]
    fn section_header_image_appears_in_band() {
        let mut doc = Document::new();
        let mut section = Section {
            header: Some(hf_image(1600, 1600)),
            ..Section::default()
        };
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("body line")));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let frag = page
            .lines
            .iter()
            .find(|l| l.image.is_some())
            .expect("header image missing");
        assert!(frag.y >= 0, "header image y {} off page", frag.y);
        assert!(
            frag.y < DEFAULT_MARGIN_HU,
            "header image y {} not in top margin",
            frag.y
        );
        let img = frag.image.as_ref().expect("payload");
        assert_eq!(img.width, 1600);
        assert_eq!(img.height, 1600);
        assert_eq!(img.bytes, MARK_PNG);
        assert_eq!(img.mime, "image/png");
        assert!(
            page.lines.iter().any(|l| l.text.contains("body line")),
            "body dropped"
        );
    }

    #[test]
    fn section_header_image_clamps_to_content_width() {
        let mut doc = Document::new();
        let mut section = Section {
            header: Some(hf_image(50_000, 10_000)),
            ..Section::default()
        };
        let content_w = section.page.content_width();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("body line")));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let frag = page
            .lines
            .iter()
            .find(|l| l.image.is_some())
            .expect("header image missing");
        assert!(
            frag.y < DEFAULT_MARGIN_HU,
            "header image y {} not in top margin",
            frag.y
        );
        assert!(
            frag.width <= content_w,
            "frag.width {} exceeds content width {}",
            frag.width,
            content_w
        );
        let img = frag.image.as_ref().expect("payload");
        assert_eq!(img.width, frag.width);
        assert!(img.width <= content_w);
        assert_eq!(
            img.height,
            (i64::from(10_000) * i64::from(img.width) / 50_000) as i32
        );
        assert!(img.height > 0);
        assert_eq!(img.bytes, MARK_PNG);
    }
}
