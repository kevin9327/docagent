//! PDF/A export from the display list. Floating-point is allowed here.

#![forbid(unsafe_code)]

use docagent_font::FontSet;
use docagent_paint::{DisplayList, Op};
use krilla::action::LinkAction;
use krilla::annotation::{Annotation, LinkAnnotation, Target};
use krilla::color::rgb;
use krilla::configure::{Archival, ConfigurationBuilder, Validator};
use krilla::geom::{PathBuilder, Point, Rect, Size, Transform};
use krilla::image::Image;
use krilla::metadata::{DateTime, Metadata};
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{Fill, LineCap, LineJoin, Stroke};
use krilla::text::{Font, TextDirection};
use krilla::{Document, SerializeSettings};

const HU_PER_PT: f32 = 100.0;
/// CSS/WeasyPrint `em,i{font-style:italic}` is a 12° slant (Pango).
/// `tan(12°)` ≈ 0.21255656. Y-down shear so glyph tops lean right around the baseline.
const ITALIC_SHEAR: f32 = 0.212_556_56;
/// Pango/WeasyPrint synthetic bold for `strong,b{font-weight:700}` when only Regular
/// is bundled: stroke width = em/24 (`pango-cairo-font.c`). 3% offset double-draw
/// reads as missing weight next to WeasyPrint.
const BOLD_STROKE_DIV: f32 = 24.0;

pub fn to_pdfa(list: &DisplayList, fonts: &FontSet) -> Result<Vec<u8>, String> {
    let font = Font::new(fonts.data.clone().into(), 0).ok_or("font")?;
    let configuration = ConfigurationBuilder::new()
        .set_validator(Validator::A(Archival::A2_B))
        .finish()
        .map_err(|e| format!("{e:?}"))?;
    let settings = SerializeSettings {
        configuration,
        ..SerializeSettings::default()
    };
    let mut document = Document::new_with(settings);
    document.set_metadata(
        Metadata::new()
            .title("DocAgent".into())
            .creator("DocAgent".into())
            .producer("DocAgent".into())
            .language("ko".into())
            .creation_date(DateTime::new(2026).month(1).day(1).hour(0).minute(0).second(0)),
    );

    let mut page_ops: Vec<Vec<Op>> = Vec::new();
    let mut cur = Vec::new();
    let mut page_size = (595.28f32, 841.89f32);
    for op in &list.ops {
        match op {
            Op::Page { width, height } => {
                if !cur.is_empty() {
                    page_ops.push(std::mem::take(&mut cur));
                }
                page_size = (*width as f32 / HU_PER_PT, *height as f32 / HU_PER_PT);
                cur.push(op.clone());
            }
            other => cur.push(other.clone()),
        }
    }
    if !cur.is_empty() {
        page_ops.push(cur);
    }
    if page_ops.is_empty() {
        let page = document.start_page_with(
            PageSettings::from_wh(page_size.0, page_size.1).ok_or("page size")?,
        );
        page.finish();
    } else {
        for ops in page_ops {
            let (w, h) = ops
                .iter()
                .find_map(|op| match op {
                    Op::Page { width, height } => {
                        Some((*width as f32 / HU_PER_PT, *height as f32 / HU_PER_PT))
                    }
                    _ => None,
                })
                .unwrap_or(page_size);
            let mut page =
                document.start_page_with(PageSettings::from_wh(w, h).ok_or("page size")?);
            let mut links = Vec::new();
            {
                let mut surface = page.surface();
                for op in &ops {
                    match op {
                        Op::FillRect { x, y, w, h, color } => {
                            if color[3] == 0 || (*w <= 0 || *h <= 0) {
                                continue;
                            }
                            if *color == [255, 255, 255, 255] {
                                continue;
                            }
                            fill_rect(
                                &mut surface,
                                *x as f32 / HU_PER_PT,
                                *y as f32 / HU_PER_PT,
                                *w as f32 / HU_PER_PT,
                                *h as f32 / HU_PER_PT,
                                *color,
                            );
                        }
                        Op::Text { .. } => {
                            draw_styled_text(&mut surface, &font, fonts, op);
                        }
                        Op::Link { x, y, w, h, uri } => {
                            if *w > 0 && *h > 0 && !uri.is_empty() {
                                links.push((*x, *y, *w, *h, uri.clone()));
                            }
                        }
                        Op::Image {
                            x,
                            y,
                            w,
                            h,
                            bytes,
                            mime,
                        } => {
                            draw_embedded_image(
                                &mut surface,
                                *x as f32 / HU_PER_PT,
                                *y as f32 / HU_PER_PT,
                                *w as f32 / HU_PER_PT,
                                *h as f32 / HU_PER_PT,
                                bytes,
                                mime,
                            );
                        }
                        Op::Page { .. } => {}
                    }
                }
                surface.finish();
            }
            for (x, y, w, h, uri) in links {
                let Some(rect) = Rect::from_xywh(
                    x as f32 / HU_PER_PT,
                    y as f32 / HU_PER_PT,
                    (w as f32 / HU_PER_PT).max(0.05),
                    (h as f32 / HU_PER_PT).max(0.05),
                ) else {
                    continue;
                };
                page.add_annotation(Annotation::new_link(
                    LinkAnnotation::new(
                        rect,
                        Target::Action(LinkAction::new(uri.clone()).into()),
                    ),
                    Some(uri),
                ));
            }
            page.finish();
        }
    }
    document.finish().map_err(|e| format!("{e:?}"))
}

fn draw_embedded_image(
    surface: &mut krilla::surface::Surface<'_>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    bytes: &[u8],
    mime: &str,
) {
    let Some(size) = Size::from_wh(w.max(0.05), h.max(0.05)) else {
        return;
    };
    let Some(image) = pdf_image(bytes, mime) else {
        return;
    };
    surface.push_transform(&Transform::from_translate(x, y));
    surface.draw_image(image, size);
    surface.pop();
}

fn pdf_image(bytes: &[u8], mime: &str) -> Option<Image> {
    let jpeg = is_jpeg(bytes, mime);
    let data = bytes.to_vec().into();
    if jpeg {
        Image::from_jpeg(data, false).ok()
    } else {
        Image::from_png(data, false)
            .ok()
            .or_else(|| Image::from_jpeg(bytes.to_vec().into(), false).ok())
    }
}

fn is_jpeg(bytes: &[u8], mime: &str) -> bool {
    mime.eq_ignore_ascii_case("image/jpeg")
        || mime.eq_ignore_ascii_case("image/jpg")
        || (bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF)
}

fn draw_styled_text(
    surface: &mut krilla::surface::Surface<'_>,
    font: &Font,
    fonts: &FontSet,
    op: &Op,
) {
    let Op::Text {
        x,
        y,
        size,
        text,
        color,
        bold,
        italic,
    } = op
    else {
        return;
    };
    let mapped = docagent_font::retain_mapped_chars(fonts, text);
    if mapped.is_empty() {
        return;
    }
    let paint = rgb::Color::new(color[0], color[1], color[2]);
    surface.set_fill(Some(Fill {
        paint: paint.into(),
        opacity: NormalizedF32::ONE,
        rule: Default::default(),
    }));
    let pt = (*size as f32 / HU_PER_PT).max(1.0);
    if *bold {
        // Fill + stroke at em/24, same as WeasyPrint/Pango fake-bold.
        surface.set_stroke(Some(Stroke {
            paint: paint.into(),
            width: (pt / BOLD_STROKE_DIV).max(0.05),
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Stroke::default()
        }));
    } else {
        surface.set_stroke(None);
    }
    let px = *x as f32 / HU_PER_PT;
    let py = *y as f32 / HU_PER_PT;
    if *italic {
        // x' = x + k*(py - y): tops (y < py) shift right.
        surface.push_transform(&Transform::from_row(
            1.0,
            0.0,
            -ITALIC_SHEAR,
            1.0,
            ITALIC_SHEAR * py,
            0.0,
        ));
    }
    if *size == docagent_layout::H1_SIZE_MAX_HU {
        draw_h1_tracked_text(surface, font, fonts, &mapped, px, py, pt);
    } else {
        surface.draw_text(
            Point::from_xy(px, py),
            font.clone(),
            pt,
            &mapped,
            false,
            TextDirection::Auto,
        );
    }
    if *italic {
        surface.pop();
    }
}

/// Letter CSS `h1{letter-spacing:-.02em}`. Krilla has no Tc; draw glyphs
/// on the same advances layout already baked so the title is not UA-loose.
fn draw_h1_tracked_text(
    surface: &mut krilla::surface::Surface<'_>,
    font: &Font,
    fonts: &FontSet,
    text: &str,
    origin_x: f32,
    y: f32,
    pt: f32,
) {
    let shaped = docagent_font::shape(fonts, text, docagent_layout::H1_SIZE_MAX_HU);
    let tracking_pt = docagent_layout::H1_LETTER_SPACING_HU as f32 / HU_PER_PT;
    let n = text.chars().count();
    let mut x = origin_x;
    for (i, ch) in text.chars().enumerate() {
        let glyph = ch.to_string();
        surface.draw_text(
            Point::from_xy(x, y),
            font.clone(),
            pt,
            &glyph,
            false,
            TextDirection::Auto,
        );
        let adv = shaped.advances.get(i).copied().unwrap_or(0) as f32 / HU_PER_PT;
        x += adv;
        if i + 1 < n {
            x += tracking_pt;
        }
    }
}

fn fill_rect(
    surface: &mut krilla::surface::Surface<'_>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: [u8; 4],
) {
    let Some(rect) = Rect::from_xywh(x, y, w.max(0.05), h.max(0.05)) else {
        return;
    };
    let mut pb = PathBuilder::new();
    pb.push_rect(rect);
    let Some(path) = pb.finish() else {
        return;
    };
    let opacity = NormalizedF32::new(f32::from(color[3]) / 255.0).unwrap_or(NormalizedF32::ONE);
    surface.set_fill(Some(Fill {
        paint: rgb::Color::new(color[0], color[1], color[2]).into(),
        opacity,
        rule: Default::default(),
    }));
    surface.set_stroke(None);
    surface.draw_path(&path);
}

pub fn claims_pdfa(bytes: &[u8]) -> bool {
    if !bytes.starts_with(b"%PDF") {
        return false;
    }
    let s = String::from_utf8_lossy(bytes);
    s.contains("PDF/A")
        || s.contains("GTS_PDFA")
        || s.contains("pdfaid")
        || s.contains("PDFA")
        || s.contains("part=\"2\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use docagent_font::FontSet;
    use docagent_layout::layout_document;
    use docagent_model::{Block, Document, Paragraph, Section};
    use docagent_paint::paint;

    #[test]
    fn pdf_starts_with_header_and_claims_pdfa() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("PDF/A fixture 한글")));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let list = paint(&tree);
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(pdf.starts_with(b"%PDF"));
        assert!(claims_pdfa(&pdf), "pdfa identifier missing");
    }

    #[test]
    fn table_pdf_paints_cell_paths() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Table(
            docagent_model::Table::from_cells(vec![vec!["a".into(), "b".into()]]),
        ));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let list = paint(&tree);
        assert!(
            list.ops.iter().any(
                |op| matches!(op, Op::FillRect { color, w, h, .. } if *color == docagent_layout::CELL_BORDER && *w > 0 && *h > 0)
            ),
            "table grid must paint WeasyPrint 1px #cbd5e1, not a sparse black hairline"
        );
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(pdf.starts_with(b"%PDF"));
        assert!(claims_pdfa(&pdf));
        assert!(pdf.len() > 1000);
    }

    #[test]
    fn table_header_paints_teal_fill() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = docagent_model::Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, w, h, .. }
                        if *color == docagent_layout::TH_BG && *w > 80 && *h > 80
                )
            }),
            "header cells must paint a teal fill"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, h, .. }
                        if *color == docagent_layout::TH_RULE
                            && *h == docagent_layout::TH_RULE_HU
                )
            }),
            "th must paint 2px #0d9488 bottom"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Text { text, color, bold: true, .. }
                        if text.contains("Hop") && *color == docagent_layout::TH_FG
                )
            }),
            "th text must be letter CSS #134e4a bold"
        );
        let marked = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        let mut plain = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Table(
            docagent_model::Table::from_cells(vec![
                vec!["Hop".into(), "File".into()],
                vec!["Input".into(), "letter.md".into()],
            ]),
        ));
        plain.sections.push(section);
        let plain_pdf = to_pdfa(
            &paint(&layout_document(&plain, &FontSet::bundled())),
            &FontSet::bundled(),
        )
        .expect("pdf");
        assert_ne!(marked, plain_pdf, "header fill must change PDF bytes");
        assert!(claims_pdfa(&marked));
    }

    #[test]
    fn letter_table_header_pdfa_uses_weasyprint_th() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = docagent_model::Table::from_cells(vec![
            vec!["Hop".into(), "File".into(), "Proof".into()],
            vec!["Input".into(), "letter.md".into(), "input SHA-256".into()],
        ]);
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        section
            .body
            .push(Block::Paragraph(strong_em_para()));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        section
            .body
            .push(Block::Paragraph(task_para("Receiving dock is clear", true)));
        let mut code = docagent_model::Run::text("prove");
        code.style.code = true;
        let mut p = Paragraph::from_text("Run ");
        p.runs.push(code);
        section.body.push(Block::Paragraph(p));
        let mut mark = Paragraph::from_text("three hashes");
        mark.runs[0].style.highlight = Some(docagent_model::Color::MARK);
        section.body.push(Block::Paragraph(mark));
        section
            .body
            .push(Block::Break(docagent_model::BreakKind::Thematic));
        let mut link = Paragraph::from_text("");
        link.runs = vec![docagent_model::Run::hyperlink(
            "DocAgent",
            "https://github.com/kevin9327/docagent",
        )];
        section.body.push(Block::Paragraph(link));
        let mut strike = Paragraph::from_text("guess");
        strike.runs[0].style.strike = true;
        section.body.push(Block::Paragraph(strike));
        let mut under = Paragraph::from_text("09:00 local");
        under.runs[0].style.underline = docagent_model::Underline::Single;
        section.body.push(Block::Paragraph(under));
        let mut pdfa = Paragraph::from_text("PDF/");
        let mut sup = docagent_model::Run::text("A");
        sup.style.superscript = true;
        pdfa.runs.push(sup);
        section.body.push(Block::Paragraph(pdfa));
        let mut h2o = Paragraph::from_text("H");
        let mut sub = docagent_model::Run::text("2");
        sub.style.subscript = true;
        h2o.runs.push(sub);
        h2o.runs.push(docagent_model::Run::text("O"));
        section.body.push(Block::Paragraph(h2o));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let th_fills = list
            .ops
            .iter()
            .filter(|op| {
                matches!(
                    op,
                    Op::FillRect { color, w, h, .. }
                        if *color == docagent_layout::TH_BG && *w > 10000 && *h > 80
                )
            })
            .count();
        assert_eq!(th_fills, 3, "letter th fill missing");
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, h, .. }
                        if *color == docagent_layout::CELL_BORDER
                            && *h == docagent_layout::CELL_BORDER_HU
                )
            }),
            "th,td 1px #cbd5e1 missing"
        );
        let th_rules = list
            .ops
            .iter()
            .filter(|op| {
                matches!(
                    op,
                    Op::FillRect { color, h, .. }
                        if *color == docagent_layout::TH_RULE
                            && *h == docagent_layout::TH_RULE_HU
                )
            })
            .count();
        assert!(th_rules >= 3, "th 2px #0d9488 missing");
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Text { text, color, bold: true, .. }
                        if text.contains("Hop") && *color == docagent_layout::TH_FG
                )
            }),
            "th text must be letter CSS #134e4a bold"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Text { text, color, bold: true, italic: false, .. }
                        if text.contains("GPU-free") && *color == docagent_layout::BODY_FG
                )
            }),
            "strong must paint letter CSS font-weight 700 in body #134e4a"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Text { text, color, italic: true, bold: false, .. }
                        if text.contains("byte-for-byte") && *color == docagent_layout::BODY_FG
                )
            }),
            "em must paint letter CSS font-style italic in body #134e4a"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Text { text, color, .. }
                        if text.contains("Replay is evidence")
                            && *color == docagent_layout::QUOTE_FG
                )
            }),
            "blockquote text must be letter CSS #134e4a"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, w, h, .. }
                        if *color == docagent_layout::QUOTE_FG
                            && *w == docagent_layout::QUOTE_BAR_W_HU
                            && *h > 200
                )
            }),
            "quote bar must not regress"
        );
        let box_hu = docagent_layout::TASK_BOX_HU;
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, w, h, .. }
                        if *color == [19, 78, 74, 255] && *w == box_hu && *h == box_hu
                )
            }),
            "task box must not regress"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, .. } if *color == docagent_layout::CODE_SPAN_BG
                )
            }),
            "code fill must not regress"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, .. } if *color == docagent_layout::MARK_BG
                )
            }),
            "mark fill must not regress"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, h, w, .. }
                        if *color == [19, 78, 74, 255]
                            && *h == docagent_layout::HR_THICK_HU
                            && *w > 10000
                )
            }),
            "hr must not regress"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, h, .. }
                        if *color == docagent_layout::LINK_COLOR
                            && *h == docagent_layout::LINK_UNDERLINE_HU
                )
            }),
            "link underline must not regress"
        );
        let guess = list.ops.iter().find_map(|op| match op {
            Op::Text { x, y, size, text, color, .. } if text.contains("guess") => {
                Some((*x, *y, *size, *color))
            }
            _ => None,
        });
        let (gx, gy, gsize, gcolor) = guess.expect("strike text");
        assert_eq!(gcolor, docagent_layout::STRIKE_COLOR);
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { x, y, h, color, .. }
                        if *color == docagent_layout::STRIKE_COLOR
                            && *h == docagent_layout::STRIKE_HU
                            && *x == gx
                            && *y == gy.saturating_sub(gsize / 3)
                )
            }),
            "strike must not regress"
        );
        let local = list.ops.iter().find_map(|op| match op {
            Op::Text { x, y, text, color, .. } if text.contains("09:00") => {
                Some((*x, *y, *color))
            }
            _ => None,
        });
        let (ux, uy, ucolor) = local.expect("u text");
        assert_eq!(
            ucolor,
            docagent_layout::BODY_FG,
            "u text must be letter body #134e4a, not UA black"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { x, y, h, color, .. }
                        if *color == docagent_layout::STRIKE_COLOR
                            && *h == docagent_layout::UNDERLINE_HU
                            && *x == ux
                            && *y == uy.saturating_add(docagent_layout::UNDERLINE_OFFSET_HU)
                )
            }),
            "underline must not regress"
        );
        assert_eq!(docagent_layout::TH_BG, [204, 251, 241, 255]);
        assert_eq!(docagent_layout::CELL_BORDER_HU, 75);
        assert_eq!(docagent_layout::TH_RULE_HU, 150);
        assert_eq!(docagent_layout::TASK_BOX_HU, 900);
        assert_eq!(docagent_layout::TASK_BORDER_HU, 150);
        assert_eq!(docagent_layout::LIST_MARK_GAP_HU, 480);
        assert_eq!(docagent_layout::CELL_PAD_X_HU, 780);
        assert_eq!(docagent_layout::CELL_SIZE_HU, 1140);
        assert_eq!(docagent_layout::IMAGE_BEFORE_HU, 720);
        assert_eq!(docagent_layout::H2_RULE_HU, 150);
        assert_eq!(docagent_layout::H2_AFTER_HU, 540);
        assert_eq!(docagent_layout::H2_BEFORE_HU, 1320);
        assert_eq!(docagent_layout::H1_AFTER_HU, 720);
        assert_eq!(docagent_layout::H1_SIZE_MAX_HU, 2100);
        assert_eq!(docagent_layout::H2_SIZE_MAX_HU, 1380);
        assert_eq!(docagent_layout::HR_THICK_HU, 225);
        assert_eq!(docagent_layout::LINK_COLOR, [13, 148, 136, 255]);
        assert_eq!(docagent_layout::STRIKE_HU, 80);
        assert_eq!(docagent_layout::UNDERLINE_HU, 80);
        assert_eq!(docagent_layout::UNDERLINE_OFFSET_HU, 100);
        assert_eq!(docagent_layout::STRIKE_COLOR, [19, 78, 74, 255]);
        assert_eq!(docagent_layout::QUOTE_FG, [19, 78, 74, 255]);
        assert_eq!(docagent_layout::BODY_FG, [19, 78, 74, 255]);
        assert_ne!(
            docagent_layout::BODY_FG,
            [0, 0, 0, 255],
            "body must not regress to UA black"
        );
        assert_eq!(docagent_layout::QUOTE_BAR_W_HU, 300);
        let pdf_run = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, .. } if text.contains("PDF") => Some((*y, *size)),
            _ => None,
        });
        let raised = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, .. } if text == "A" => Some((*y, *size)),
            _ => None,
        });
        let (py, psize) = pdf_run.expect("PDF");
        let (ay, asize) = raised.expect("super");
        assert_eq!(asize, docagent_layout::SUPER_SUB_SIZE_HU);
        assert_eq!(asize, docagent_layout::super_sub_size(psize));
        assert_eq!(
            ay,
            py.saturating_sub(docagent_layout::SUPER_SUB_SHIFT_HU),
            "super must not regress"
        );
        let h = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, .. } if text == "H" => Some((*y, *size)),
            _ => None,
        });
        let lowered = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, .. } if text == "2" => Some((*y, *size)),
            _ => None,
        });
        let (hy, hsize) = h.expect("H");
        let (ty, tsize) = lowered.expect("sub");
        assert_eq!(tsize, docagent_layout::SUPER_SUB_SIZE_HU);
        assert_eq!(tsize, docagent_layout::super_sub_size(hsize));
        assert_eq!(
            ty,
            hy.saturating_add(docagent_layout::SUPER_SUB_SHIFT_HU),
            "sub must not regress"
        );
        assert_eq!(docagent_layout::SUPER_SUB_SIZE_HU, 750);
        assert_eq!(docagent_layout::SUPER_SUB_SHIFT_HU, 375);
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(claims_pdfa(&pdf));
        let mut plain = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Table(
            docagent_model::Table::from_cells(vec![
                vec!["Hop".into(), "File".into(), "Proof".into()],
                vec!["Input".into(), "letter.md".into(), "input SHA-256".into()],
            ]),
        ));
        plain.sections.push(section);
        let plain_pdf = to_pdfa(
            &paint(&layout_document(&plain, &FontSet::bundled())),
            &FontSet::bundled(),
        )
        .expect("pdf");
        assert_ne!(pdf, plain_pdf, "th fill/rule must change PDF bytes vs sparse grid");
    }

    #[test]
    fn heading_rule_paints_non_grid_fill() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.size = 1800;
        h1.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h1));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let list = paint(&tree);
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, h, w, .. }
                        if *color == docagent_layout::BODY_FG
                            && *h == docagent_layout::H1_RULE_HU
                            && *w > 1000
                )
            }),
            "heading rule must paint a non-table fill"
        );
        let with_rule = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        let mut plain = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("Northwind Freight")));
        plain.sections.push(section);
        let plain_pdf = to_pdfa(
            &paint(&layout_document(&plain, &FontSet::bundled())),
            &FontSet::bundled(),
        )
        .expect("pdf");
        assert_ne!(with_rule, plain_pdf, "heading rule must change PDF bytes");
        assert!(claims_pdfa(&with_rule));
    }

    #[test]
    fn bullet_marker_paints_non_grid_fill() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Receiving dock is clear");
        p.numbering = Some(docagent_model::NumberingRef {
            definition_id: 0,
            level: 0,
            start: None,
            format: Some(docagent_model::NumberFormat::Bullet),
        });
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let list = paint(&tree);
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, h, w, .. }
                        if *color == [19, 78, 74, 255] && *h == *w && *h >= 160 && *h < 1000
                )
            }),
            "bullet marker must paint a square fill"
        );
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(claims_pdfa(&pdf));
    }

    #[test]
    fn superscript_shrinks_text() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("PDF/");
        let mut sup = docagent_model::Run::text("A");
        sup.style.superscript = true;
        p.runs.push(sup);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let base = list.ops.iter().find_map(|op| match op {
            Op::Text { text, size, .. } if text.contains("PDF") => Some(*size),
            _ => None,
        });
        let raised = list.ops.iter().find_map(|op| match op {
            Op::Text { text, size, y, .. } if text == "A" => Some((*size, *y)),
            _ => None,
        });
        let Some(base) = base else {
            panic!("base text missing: {:?}", list.ops);
        };
        let Some((rs, ry)) = raised else {
            panic!("super text missing: {:?}", list.ops);
        };
        assert_eq!(
            rs,
            docagent_layout::SUPER_SUB_SIZE_HU,
            "super size {rs} must be letter CSS 0.75em, not missing"
        );
        assert_eq!(rs, docagent_layout::super_sub_size(base));
        let base_y = list.ops.iter().find_map(|op| match op {
            Op::Text { text, y, .. } if text.contains("PDF") => Some(*y),
            _ => None,
        });
        assert_eq!(
            ry,
            base_y
                .expect("base y")
                .saturating_sub(docagent_layout::SUPER_SUB_SHIFT_HU),
            "super y {ry} must rise WeasyPrint 0.5em"
        );
        assert_eq!(docagent_layout::SUPER_SUB_SIZE_HU, 750);
        assert_eq!(docagent_layout::SUPER_SUB_SHIFT_HU, 375);
        let marked = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        let mut plain = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("PDF/A")));
        plain.sections.push(section);
        let plain_pdf = to_pdfa(
            &paint(&layout_document(&plain, &FontSet::bundled())),
            &FontSet::bundled(),
        )
        .expect("pdf");
        assert_ne!(marked, plain_pdf, "superscript must change PDF bytes");
        assert!(claims_pdfa(&marked));
    }

    #[test]
    fn subscript_shrinks_and_lowers_text() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("H");
        let mut sub = docagent_model::Run::text("2");
        sub.style.subscript = true;
        p.runs.push(sub);
        p.runs.push(docagent_model::Run::text("O"));
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let base = list.ops.iter().find_map(|op| match op {
            Op::Text { text, size, .. } if text == "H" => Some(*size),
            _ => None,
        });
        let lowered = list.ops.iter().find_map(|op| match op {
            Op::Text { text, size, y, .. } if text == "2" => Some((*size, *y)),
            _ => None,
        });
        let Some(base) = base else {
            panic!("base text missing: {:?}", list.ops);
        };
        let Some((rs, ry)) = lowered else {
            panic!("sub text missing: {:?}", list.ops);
        };
        assert_eq!(
            rs,
            docagent_layout::SUPER_SUB_SIZE_HU,
            "sub size {rs} must be letter CSS 0.75em, not missing"
        );
        assert_eq!(rs, docagent_layout::super_sub_size(base));
        let base_y = list.ops.iter().find_map(|op| match op {
            Op::Text { text, y, .. } if text == "H" => Some(*y),
            _ => None,
        });
        assert_eq!(
            ry,
            base_y
                .expect("base y")
                .saturating_add(docagent_layout::SUPER_SUB_SHIFT_HU),
            "sub y {ry} must drop WeasyPrint 0.5em, not 1/5em"
        );
        assert_eq!(docagent_layout::SUPER_SUB_SIZE_HU, 750);
        assert_eq!(docagent_layout::SUPER_SUB_SHIFT_HU, 375);
        let marked = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        let mut plain = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("H2O")));
        plain.sections.push(section);
        let plain_pdf = to_pdfa(
            &paint(&layout_document(&plain, &FontSet::bundled())),
            &FontSet::bundled(),
        )
        .expect("pdf");
        assert_ne!(marked, plain_pdf, "subscript must change PDF bytes");
        assert!(claims_pdfa(&marked));
    }

    #[test]
    fn underline_paints_non_grid_fill() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("09:00 local");
        p.runs[0].style.underline = docagent_model::Underline::Single;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, h, w, .. }
                        if *color == docagent_layout::STRIKE_COLOR
                            && *h == docagent_layout::UNDERLINE_HU
                            && *w > 80
                )
            }),
            "underline must paint a fill"
        );
        let marked = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        let mut plain = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("09:00 local")));
        plain.sections.push(section);
        let plain_pdf = to_pdfa(
            &paint(&layout_document(&plain, &FontSet::bundled())),
            &FontSet::bundled(),
        )
        .expect("pdf");
        assert_ne!(marked, plain_pdf, "underline must change PDF bytes");
        assert!(claims_pdfa(&marked));
        let s = String::from_utf8_lossy(&marked);
        assert!(!s.contains("/URI"), "plain underline must not be a /Link");
    }

    #[test]
    fn hyperlink_underline_paints_non_grid_fill() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![docagent_model::Run::hyperlink(
            "DocAgent",
            "https://github.com/kevin9327/docagent",
        )];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, h, w, .. }
                        if *color == docagent_layout::LINK_COLOR
                            && *h == docagent_layout::LINK_UNDERLINE_HU
                            && *w > 80
                )
            }),
            "hyperlink underline must paint a fill"
        );
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(claims_pdfa(&pdf));
        let s = String::from_utf8_lossy(&pdf);
        assert!(s.contains("/URI"), "PDF /URI action missing");
        assert!(
            s.contains("https://github.com/kevin9327/docagent"),
            "PDF URI target missing"
        );
        assert!(s.contains("/Link"), "PDF /Link subtype missing");
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Text { text, color, .. }
                        if text.contains("DocAgent") && *color == docagent_layout::LINK_COLOR
                )
            }),
            "link glyphs must be teal: {:?}",
            list.ops
        );
        let mut plain = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("DocAgent")));
        plain.sections.push(section);
        let plain_pdf = to_pdfa(
            &paint(&layout_document(&plain, &FontSet::bundled())),
            &FontSet::bundled(),
        )
        .expect("pdf");
        assert_ne!(pdf, plain_pdf, "teal link text must change PDF bytes");
    }

    #[test]
    fn highlight_paints_non_grid_fill() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("hashes");
        p.runs[0].style.highlight = Some(docagent_model::Color::MARK);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, h, w, .. }
                        if *color == [253, 230, 138, 255] && *w > 80 && *h > 80
                )
            }),
            "highlight must paint an amber fill"
        );
        let marked = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        let mut plain = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("hashes")));
        plain.sections.push(section);
        let plain_pdf = to_pdfa(
            &paint(&layout_document(&plain, &FontSet::bundled())),
            &FontSet::bundled(),
        )
        .expect("pdf");
        assert_ne!(marked, plain_pdf, "highlight must change PDF bytes");
        assert!(claims_pdfa(&marked));
    }

    fn strong_em_para() -> Paragraph {
        let mut p = Paragraph::from_text("one convert, ");
        let mut strong = docagent_model::Run::text("GPU-free");
        strong.style.bold = true;
        p.runs.push(strong);
        p.runs.push(docagent_model::Run::text(". match "));
        let mut em = docagent_model::Run::text("byte-for-byte");
        em.style.italic = true;
        p.runs.push(em);
        p
    }

    fn task_para(text: &str, checked: bool) -> Paragraph {
        let mut p = Paragraph::from_text(text);
        p.space_after = 80;
        p.numbering = Some(docagent_model::NumberingRef {
            definition_id: 2,
            level: 0,
            start: checked.then_some(1),
            format: Some(docagent_model::NumberFormat::Task),
        });
        p
    }

    #[test]
    fn task_box_paints_non_grid_fill() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(task_para("Receiving dock is clear", true)));
        section
            .body
            .push(Block::Paragraph(task_para("Replay the convert", false)));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let teal = [19, 78, 74, 255];
        let box_hu = docagent_layout::TASK_BOX_HU;
        let border_hu = docagent_layout::TASK_BORDER_HU;
        let gap_hu = docagent_layout::LIST_MARK_GAP_HU;
        let dock = list.ops.iter().find_map(|op| match op {
            Op::Text { x, text, .. } if text.contains("dock") => Some(*x),
            _ => None,
        });
        let replay = list.ops.iter().find_map(|op| match op {
            Op::Text { x, text, .. } if text.contains("Replay") => Some(*x),
            _ => None,
        });
        let dx = dock.expect("checked text");
        let rx = replay.expect("unchecked text");
        let checked = list.ops.iter().find_map(|op| match op {
            Op::FillRect { x, w, h, color, .. }
                if *color == teal && *w == box_hu && *h == box_hu =>
            {
                Some(*x)
            }
            _ => None,
        });
        let cx = checked.expect("checked - [x] 0.9em fill");
        assert_eq!(
            cx,
            docagent_layout::PAGE_MARGIN_X_HU.saturating_add(docagent_layout::LIST_INDENT_HU),
            "PDF task checkbox x must be letter CSS ul.tasks padding-left 1.25rem, not a hanging marker"
        );
        assert_eq!(
            dx,
            cx.saturating_add(box_hu).saturating_add(gap_hu),
            "PDF checked text gap must be letter CSS 0.4rem"
        );
        assert_eq!(
            dx,
            cx.saturating_add(docagent_layout::TASK_MARK_ADVANCE_HU),
            "PDF task text must sit 0.9em + 0.4rem after the in-flow box"
        );
        assert_eq!(docagent_layout::TASK_MARK_ADVANCE_HU, 1380);
        let horiz = list
            .ops
            .iter()
            .filter(|op| {
                matches!(
                    op,
                    Op::FillRect { w, h, color, .. }
                        if *color == teal && *w == box_hu && *h == border_hu
                )
            })
            .count();
        let vert = list
            .ops
            .iter()
            .filter(|op| {
                matches!(
                    op,
                    Op::FillRect { w, h, color, .. }
                        if *color == teal && *w == border_hu && *h == box_hu
                )
            })
            .count();
        assert_eq!(horiz, 2, "PDF unchecked - [ ] must paint 1.5pt top/bottom");
        assert_eq!(vert, 2, "PDF unchecked - [ ] must paint 1.5pt left/right");
        let open_x = list.ops.iter().find_map(|op| match op {
            Op::FillRect { x, w, h, color, .. }
                if *color == teal && *w == box_hu && *h == border_hu =>
            {
                Some(*x)
            }
            _ => None,
        });
        assert_eq!(
            rx,
            open_x
                .expect("unchecked box")
                .saturating_add(box_hu)
                .saturating_add(gap_hu),
            "PDF unchecked text gap must be letter CSS 0.4rem"
        );
        assert_eq!(docagent_layout::TASK_BOX_HU, 900);
        assert_eq!(docagent_layout::TASK_BORDER_HU, 150);
        assert_eq!(docagent_layout::LIST_MARK_GAP_HU, 480);
        assert_eq!(docagent_layout::CODE_SPAN_PAD_X_HU, 350);
        assert_eq!(docagent_layout::MARK_PAD_X_HU, 200);
        assert_eq!(docagent_layout::HR_THICK_HU, 225);
        assert_eq!(docagent_layout::LINK_COLOR, [13, 148, 136, 255]);
        assert_eq!(docagent_layout::H1_SIZE_MAX_HU, 2100);
        assert_eq!(docagent_layout::H2_SIZE_MAX_HU, 1380);
        assert_eq!(docagent_layout::CELL_PAD_X_HU, 780);
        assert_eq!(docagent_layout::LIST_ITEM_AFTER_HU, 420);
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(claims_pdfa(&pdf));
        let mut plain = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("Receiving dock is clear")));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("Replay the convert")));
        plain.sections.push(section);
        let plain_pdf = to_pdfa(
            &paint(&layout_document(&plain, &FontSet::bundled())),
            &FontSet::bundled(),
        )
        .expect("plain");
        assert_ne!(pdf, plain_pdf, "task checkboxes must change PDF bytes");
        let mut open_only = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(task_para("Receiving dock is clear", false)));
        section
            .body
            .push(Block::Paragraph(task_para("Replay the convert", false)));
        open_only.sections.push(section);
        let open_pdf = to_pdfa(
            &paint(&layout_document(&open_only, &FontSet::bundled())),
            &FontSet::bundled(),
        )
        .expect("open");
        assert_ne!(pdf, open_pdf, "checked - [x] fill must change PDF bytes");
    }

    #[test]
    fn code_block_paints_non_grid_fill() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("docagent prove examples/letter.md");
        p.code_block = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, h, w, .. }
                        if *color == [204, 251, 241, 255] && *w > 10000 && *h > 80
                )
            }),
            "code block must paint a wide fill"
        );
        let coded = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        let mut plain = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Paragraph(Paragraph::from_text(
            "docagent prove examples/letter.md",
        )));
        plain.sections.push(section);
        let plain_pdf = to_pdfa(
            &paint(&layout_document(&plain, &FontSet::bundled())),
            &FontSet::bundled(),
        )
        .expect("pdf");
        assert_ne!(coded, plain_pdf, "code block must change PDF bytes");
        assert!(claims_pdfa(&coded));
    }

    #[test]
    fn thematic_rule_paints_non_grid_fill() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Break(docagent_model::BreakKind::Thematic));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, h, w, .. }
                        if *color == [19, 78, 74, 255]
                            && *h == docagent_layout::HR_THICK_HU
                            && *w > 10000
                )
            }),
            "thematic rule must paint a wide fill"
        );
        let ruled = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        let mut plain = Document::new();
        plain.sections.push(Section::default());
        let plain_pdf = to_pdfa(
            &paint(&layout_document(&plain, &FontSet::bundled())),
            &FontSet::bundled(),
        )
        .expect("pdf");
        assert_ne!(ruled, plain_pdf, "thematic rule must change PDF bytes");
        assert!(claims_pdfa(&ruled));
    }

    #[test]
    fn letter_hr_pdfa_uses_weasyprint_3px_11rem() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut before = Paragraph::from_text("output SHA-256");
        before.space_after = 200;
        section.body.push(Block::Paragraph(before));
        section
            .body
            .push(Block::Break(docagent_model::BreakKind::Thematic));
        let mut after = Paragraph::from_text("Please confirm before 09:00 local");
        after.space_after = 200;
        section.body.push(Block::Paragraph(after));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let before = list.ops.iter().find_map(|op| match op {
            Op::Text { y, text, .. } if text.contains("SHA-256") => Some(*y),
            _ => None,
        });
        let rule = list.ops.iter().find_map(|op| match op {
            Op::FillRect { y, h, w, color, .. }
                if *color == [19, 78, 74, 255]
                    && *h == docagent_layout::HR_THICK_HU
                    && *w > 10000 =>
            {
                Some((*y, *h))
            }
            _ => None,
        });
        let after = list.ops.iter().find_map(|op| match op {
            Op::Text { y, text, .. } if text.contains("confirm") => Some(*y),
            _ => None,
        });
        let y0 = before.expect("before");
        let (ry, rh) = rule.expect("hr");
        let y2 = after.expect("after");
        assert_eq!(rh, docagent_layout::HR_THICK_HU);
        assert!(
            ry >= y0.saturating_add(docagent_layout::HR_BEFORE_HU),
            "hr y {ry} jammed vs letter CSS 1.1rem after {y0}"
        );
        assert!(
            y2 >= ry
                .saturating_add(docagent_layout::HR_THICK_HU)
                .saturating_add(docagent_layout::HR_AFTER_HU),
            "after hr {y2} jammed vs letter CSS 1.1rem"
        );
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(claims_pdfa(&pdf));
        let mut thin = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Paragraph(Paragraph::from_text("output SHA-256")));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("Please confirm before 09:00 local")));
        thin.sections.push(section);
        let thin_pdf = to_pdfa(
            &paint(&layout_document(&thin, &FontSet::bundled())),
            &FontSet::bundled(),
        )
        .expect("pdf");
        assert_ne!(pdf, thin_pdf, "3px hr must change PDF bytes vs no rule");
    }

    #[test]
    fn letter_link_pdfa_uses_weasyprint_teal_underline() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![
            docagent_model::Run::hyperlink(
                "DocAgent",
                "https://github.com/kevin9327/docagent",
            ),
            docagent_model::Run::text(" runtime"),
        ];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Text { text, color, .. }
                        if text.contains("DocAgent") && *color == docagent_layout::LINK_COLOR
                )
            }),
            "link glyphs must be letter CSS #0d9488"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, h, w, .. }
                        if *color == docagent_layout::LINK_COLOR
                            && *h == docagent_layout::LINK_UNDERLINE_HU
                            && *w > 80
                )
            }),
            "link underline must paint WeasyPrint teal"
        );
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(claims_pdfa(&pdf));
        let s = String::from_utf8_lossy(&pdf);
        assert!(s.contains("/URI"), "PDF /URI action missing");
        assert!(s.contains("/Link"), "PDF /Link subtype missing");
        assert!(s.contains("https://github.com/kevin9327/docagent"));
        let mut plain = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("DocAgent runtime")));
        plain.sections.push(section);
        let plain_pdf = to_pdfa(
            &paint(&layout_document(&plain, &FontSet::bundled())),
            &FontSet::bundled(),
        )
        .expect("pdf");
        assert_ne!(pdf, plain_pdf, "teal link underline must change PDF bytes");
    }

    #[test]
    fn letter_inline_code_pdfa_uses_weasyprint_padding() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Run ");
        let mut code = docagent_model::Run::text("prove");
        code.style.code = true;
        p.runs.push(code);
        p.runs.push(docagent_model::Run::text(" twice"));
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let prove = list.ops.iter().find_map(|op| match op {
            Op::Text { x, y, size, text, color, .. } if text.contains("prove") => {
                Some((*x, *y, *size, *color))
            }
            _ => None,
        });
        let twice = list.ops.iter().find_map(|op| match op {
            Op::Text { x, text, color, .. } if text.contains("twice") => Some((*x, *color)),
            _ => None,
        });
        let (px, py, psize, pcolor) = prove.expect("prove");
        let (tx, tcolor) = twice.expect("twice");
        assert_eq!(pcolor, docagent_layout::CODE_SPAN_COLOR);
        assert_eq!(
            tcolor,
            docagent_layout::BODY_FG,
            "body beside code must be letter #134e4a, not UA black"
        );
        let fill = list.ops.iter().find_map(|op| match op {
            Op::FillRect { x, y, w, h, color }
                if *color == docagent_layout::CODE_SPAN_BG && *w > 80 && *h > 80 =>
            {
                Some((*x, *y, *w, *h))
            }
            _ => None,
        });
        let (fx, fy, fw, fh) = fill.expect("code fill");
        assert_eq!(fx, px.saturating_sub(docagent_layout::CODE_SPAN_PAD_X_HU));
        let glyph_w = tx
            .saturating_sub(px)
            .saturating_sub(docagent_layout::CODE_SPAN_PAD_X_HU);
        assert_eq!(
            fw,
            glyph_w.saturating_add(docagent_layout::CODE_SPAN_PAD_X_HU.saturating_mul(2))
        );
        assert_eq!(
            fy,
            py.saturating_sub(psize)
                .saturating_sub(docagent_layout::CODE_SPAN_PAD_Y_HU)
        );
        assert_eq!(
            fh,
            psize.saturating_add(docagent_layout::CODE_SPAN_PAD_Y_HU.saturating_mul(2))
        );
        assert_eq!(docagent_layout::CODE_SPAN_PAD_X_HU, 350);
        assert_eq!(docagent_layout::H1_SIZE_MAX_HU, 2100);
        assert_eq!(docagent_layout::HR_THICK_HU, 225);
        assert_eq!(docagent_layout::LINK_COLOR, [13, 148, 136, 255]);
        assert_eq!(docagent_layout::LIST_ITEM_AFTER_HU, 420);
        assert_eq!(docagent_layout::QUOTE_BAR_W_HU, 300);
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(claims_pdfa(&pdf));
        let mut plain = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("Run prove twice")));
        plain.sections.push(section);
        let plain_pdf = to_pdfa(
            &paint(&layout_document(&plain, &FontSet::bundled())),
            &FontSet::bundled(),
        )
        .expect("pdf");
        assert_ne!(pdf, plain_pdf, "inline code pad/color must change PDF bytes");
    }

    #[test]
    fn letter_kbd_samp_pdfa_uses_weasyprint_code_chip() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Run ");
        let mut kbd = docagent_model::Run::text("prove");
        kbd.style.code = true;
        p.runs.push(kbd);
        p.runs.push(docagent_model::Run::text(" twice. Output "));
        let mut samp = docagent_model::Run::text("SHA-256");
        samp.style.code = true;
        p.runs.push(samp);
        p.runs.push(docagent_model::Run::text("."));
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let prove = list.ops.iter().find_map(|op| match op {
            Op::Text { x, y, size, text, color, .. } if *text == "prove" => {
                Some((*x, *y, *size, *color))
            }
            _ => None,
        });
        let hash = list.ops.iter().find_map(|op| match op {
            Op::Text { x, size, text, color, .. } if text.contains("SHA-256") => {
                Some((*x, *size, *color))
            }
            _ => None,
        });
        let (px, py, psize, pcolor) = prove.expect("kbd");
        let (hx, hsize, hcolor) = hash.expect("samp");
        assert_eq!(pcolor, docagent_layout::CODE_SPAN_COLOR);
        assert_eq!(hcolor, docagent_layout::CODE_SPAN_COLOR);
        assert_eq!(psize, docagent_layout::CODE_SPAN_SIZE_HU);
        assert_eq!(hsize, docagent_layout::CODE_SPAN_SIZE_HU);
        assert!(hx > px);
        let fills: Vec<_> = list
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::FillRect { x, y, w, h, color }
                    if *color == docagent_layout::CODE_SPAN_BG && *w > 80 && *h > 80 =>
                {
                    Some((*x, *y, *w, *h))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            fills.len(),
            2,
            "PDF kbd and samp must each paint a mint chip: {:?}",
            list.ops
        );
        let (fx, fy, fw, fh) = fills[0];
        assert_eq!(fx, px.saturating_sub(docagent_layout::CODE_SPAN_PAD_X_HU));
        assert_eq!(
            fy,
            py.saturating_sub(psize)
                .saturating_sub(docagent_layout::CODE_SPAN_PAD_Y_HU)
        );
        assert_eq!(
            fh,
            psize.saturating_add(docagent_layout::CODE_SPAN_PAD_Y_HU.saturating_mul(2))
        );
        assert!(fw > psize);
        assert_eq!(
            fills[1].0,
            hx.saturating_sub(docagent_layout::CODE_SPAN_PAD_X_HU)
        );
        assert!(
            !list.ops.iter().any(|op| matches!(
                op,
                Op::FillRect { w, h, color, .. }
                    if (*w == docagent_layout::CELL_BORDER_HU
                        || *h == docagent_layout::CELL_BORDER_HU)
                        && *color != docagent_layout::CODE_SPAN_BG
            )),
            "PDF kbd/samp must not keep WeasyPrint UA 1px border"
        );
        assert_eq!(docagent_layout::CODE_SPAN_PAD_X_HU, 350);
        assert_eq!(docagent_layout::CODE_SPAN_SIZE_HU, 920);
        assert_eq!(docagent_layout::CODE_PAD_X_HU, 1200);
        assert_eq!(docagent_layout::H1_SIZE_MAX_HU, 2100);
        assert_eq!(docagent_layout::H2_AFTER_HU, 540);
        assert_eq!(docagent_layout::TASK_MARK_ADVANCE_HU, 1380);
        assert_eq!(docagent_layout::LIST_NESTED_GAP_HU, 300);
        assert_eq!(docagent_layout::PAGE_MARGIN_X_HU, 3300);
        assert_eq!(docagent_layout::QUOTE_BAR_W_HU, 300);
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(claims_pdfa(&pdf));
        let mut plain = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Paragraph(Paragraph::from_text(
            "Run prove twice. Output SHA-256.",
        )));
        plain.sections.push(section);
        let plain_pdf = to_pdfa(
            &paint(&layout_document(&plain, &FontSet::bundled())),
            &FontSet::bundled(),
        )
        .expect("pdf");
        assert_ne!(
            pdf, plain_pdf,
            "kbd/samp mint chips must change PDF bytes vs UA bare keys"
        );
    }

    #[test]
    fn letter_abbr_underline_pdfa_uses_weasyprint_dotted_teal() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Replay ");
        let mut abbr = docagent_model::Run::text("PDF/A");
        abbr.style.underline = docagent_model::Underline::Dotted;
        p.runs.push(abbr);
        p.runs.push(docagent_model::Run::text(" hashes."));
        section.body.push(Block::Paragraph(p));
        let mut under = Paragraph::from_text("09:00 local");
        under.runs[0].style.underline = docagent_model::Underline::Single;
        section.body.push(Block::Paragraph(under));
        doc.sections.push(section);
        let fonts = FontSet::bundled();
        let list = paint(&layout_document(&doc, &fonts));
        let pdfa = list.ops.iter().find_map(|op| match op {
            Op::Text { x, y, size, text, color, .. } if text.contains("PDF/A") => {
                Some((*x, *y, *size, *color))
            }
            _ => None,
        });
        let (ax, ay, asize, acolor) = pdfa.expect("abbr");
        assert_eq!(acolor, docagent_layout::BODY_FG);
        let offset = docagent_layout::dotted_underline_offset(asize);
        assert_eq!(offset, docagent_layout::ABBR_UNDERLINE_OFFSET_HU);
        let dots: Vec<_> = list
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::FillRect { x, y, w, h, color }
                    if *color == docagent_layout::BODY_FG
                        && *h == docagent_layout::UNDERLINE_HU
                        && *y == ay.saturating_add(offset)
                        && *x >= ax =>
                {
                    Some((*x, *w))
                }
                _ => None,
            })
            .collect();
        assert!(
            dots.len() >= 2,
            "PDF abbr must be 1px on/off dots, not a solid UA underline: {:?}",
            list.ops
        );
        assert_eq!(dots[0].0, ax);
        assert!(
            dots.iter().all(|(_, w)| *w <= docagent_layout::DOTTED_UNDERLINE_DOT_HU),
            "PDF abbr dots must stay 1px: {dots:?}"
        );
        assert!(
            !list.ops.iter().any(|op| matches!(
                op,
                Op::FillRect { x, w, h, color, .. }
                    if *color == docagent_layout::BODY_FG
                        && *h == docagent_layout::UNDERLINE_HU
                        && *x == ax
                        && *w > docagent_layout::DOTTED_UNDERLINE_DOT_HU.saturating_mul(2)
            )),
            "PDF abbr must not keep a solid underline"
        );
        assert_eq!(docagent_layout::ABBR_UNDERLINE_OFFSET_HU, 150);
        assert_eq!(docagent_layout::DOTTED_UNDERLINE_DOT_HU, 80);
        assert_eq!(docagent_layout::CODE_SPAN_PAD_X_HU, 350);
        assert_eq!(docagent_layout::CODE_SPAN_SIZE_HU, 920);
        assert_eq!(docagent_layout::CODE_PAD_X_HU, 1200);
        assert_eq!(docagent_layout::H1_SIZE_MAX_HU, 2100);
        assert_eq!(docagent_layout::H2_AFTER_HU, 540);
        assert_eq!(docagent_layout::HEADING_LEADING_PCT, 120);
        assert_eq!(docagent_layout::TASK_MARK_ADVANCE_HU, 1380);
        assert_eq!(docagent_layout::LIST_NESTED_GAP_HU, 300);
        assert_eq!(docagent_layout::PAGE_MARGIN_X_HU, 3300);
        assert_eq!(docagent_layout::QUOTE_BAR_W_HU, 300);
        let pdf = to_pdfa(&list, &fonts).expect("pdf");
        assert!(claims_pdfa(&pdf));
        let mut solid = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Replay ");
        let mut under = docagent_model::Run::text("PDF/A");
        under.style.underline = docagent_model::Underline::Single;
        p.runs.push(under);
        p.runs.push(docagent_model::Run::text(" hashes."));
        section.body.push(Block::Paragraph(p));
        solid.sections.push(section);
        let solid_pdf = to_pdfa(
            &paint(&layout_document(&solid, &fonts)),
            &fonts,
        )
        .expect("pdf");
        assert_ne!(
            pdf, solid_pdf,
            "abbr dotted teal must change PDF bytes vs a solid u underline"
        );
    }

    fn decimal_para(text: &str, start: u32) -> Paragraph {
        let mut p = Paragraph::from_text(text);
        p.space_after = 80;
        p.numbering = Some(docagent_model::NumberingRef {
            definition_id: 1,
            level: 0,
            start: Some(start),
            format: Some(docagent_model::NumberFormat::Decimal),
        });
        p
    }

    #[test]
    fn letter_list_marker_pdfa_uses_weasyprint_teal() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(decimal_para("Hash the input", 1)));
        section
            .body
            .push(Block::Paragraph(decimal_para("Run Convert", 2)));
        section
            .body
            .push(Block::Paragraph(bullet_para("PDF/A", 1)));
        section
            .body
            .push(Block::Paragraph(decimal_para("Compare the capsule", 3)));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        doc.sections.push(section);
        let fonts = FontSet::bundled();
        let list = paint(&layout_document(&doc, &fonts));
        let one = list.ops.iter().find_map(|op| match op {
            Op::Text { text, color, .. } if text.starts_with("1. ") => Some(*color),
            _ => None,
        });
        let two = list.ops.iter().find_map(|op| match op {
            Op::Text { text, color, .. } if text.starts_with("2. ") => Some(*color),
            _ => None,
        });
        let three = list.ops.iter().find_map(|op| match op {
            Op::Text { text, color, .. } if text.starts_with("3. ") => Some(*color),
            _ => None,
        });
        let hash = list.ops.iter().find_map(|op| match op {
            Op::Text { text, color, .. } if text.contains("Hash the input") => Some(*color),
            _ => None,
        });
        let date = list.ops.iter().find_map(|op| match op {
            Op::Text { text, color, .. } if text.contains("September") => Some(*color),
            _ => None,
        });
        assert_eq!(
            one.expect("1. marker"),
            docagent_layout::MARKER_FG,
            "PDF ol ::marker must be letter CSS #134e4a, not UA CanvasText black"
        );
        assert_eq!(two.expect("2. marker"), docagent_layout::MARKER_FG);
        assert_eq!(three.expect("3. marker"), docagent_layout::MARKER_FG);
        assert_ne!(one.expect("1. marker"), [0, 0, 0, 255]);
        assert_eq!(hash.expect("hash item"), docagent_layout::BODY_FG);
        assert_eq!(date.expect("body"), docagent_layout::BODY_FG);
        assert!(
            list.ops.iter().any(|op| matches!(
                op,
                Op::FillRect { color, w, h, .. }
                    if *color == docagent_layout::MARKER_FG && *w == *h && *w >= 400
            )),
            "PDF nested disc ::marker must be letter #134e4a: {:?}",
            list.ops
        );
        assert!(
            !list.ops.iter().any(|op| matches!(
                op,
                Op::FillRect { color, w, h, .. }
                    if *color == [0, 0, 0, 255] && *w == *h && *w >= 400
            )),
            "PDF nested disc must not stay UA CanvasText black"
        );
        assert_eq!(docagent_layout::MARKER_FG, docagent_layout::BODY_FG);
        assert_eq!(docagent_layout::MARKER_FG, [19, 78, 74, 255]);
        assert_eq!(docagent_layout::ABBR_UNDERLINE_OFFSET_HU, 150);
        assert_eq!(docagent_layout::DOTTED_UNDERLINE_DOT_HU, 80);
        assert_eq!(docagent_layout::CODE_PAD_X_HU, 1200);
        assert_eq!(docagent_layout::HEADING_LEADING_PCT, 120);
        assert_eq!(docagent_layout::H2_RULE_HU, 150);
        assert_eq!(docagent_layout::CELL_BORDER_HU, 75);
        assert_eq!(docagent_layout::PAGE_MARGIN_X_HU, 3300);
        assert_eq!(docagent_layout::PAGE_MARGIN_Y_HU, 3000);
        assert_eq!(docagent_layout::TASK_MARK_ADVANCE_HU, 1380);
        assert_eq!(docagent_layout::H1_LETTER_SPACING_HU, -42);
        assert_eq!(docagent_layout::LIST_NESTED_GAP_HU, 300);
        assert_eq!(docagent_layout::CODE_SPAN_SIZE_HU, 920);
        assert_eq!(docagent_layout::H1_AFTER_HU, 720);
        assert_eq!(docagent_layout::H2_BEFORE_HU, 1320);
        assert_eq!(docagent_layout::H2_AFTER_HU, 540);
        assert_eq!(docagent_layout::BODY_LEADING_PCT, 145);
        assert_eq!(docagent_layout::BODY_FG, [19, 78, 74, 255]);
        assert_eq!(docagent_layout::QUOTE_BAR_W_HU, 300);
        assert_eq!(
            docagent_layout::cell_valign_offset(docagent_model::VAlign::Top, 2000, 5000),
            0
        );
        let pdf = to_pdfa(&list, &fonts).expect("pdf");
        assert!(claims_pdfa(&pdf));
        let mut plain = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("Hash the input")));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("Run Convert")));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("PDF/A")));
        section.body.push(Block::Paragraph(Paragraph::from_text(
            "Compare the capsule",
        )));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        plain.sections.push(section);
        let plain_pdf = to_pdfa(
            &paint(&layout_document(&plain, &fonts)),
            &fonts,
        )
        .expect("pdf");
        assert_ne!(
            pdf, plain_pdf,
            "teal ::marker must change PDF bytes vs UA unnumbered CanvasText"
        );
    }

    fn pre_code_para(text: &str) -> Paragraph {
        let mut p = Paragraph::from_text(text);
        p.code_block = true;
        p.space_before = 80;
        p.space_after = 200;
        for run in &mut p.runs {
            run.style.code = true;
        }
        p
    }

    #[test]
    fn letter_pre_code_pdfa_is_weasyprint_transparent_zero_pad() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut inline = Paragraph::from_text("Run ");
        let mut prove = docagent_model::Run::text("prove");
        prove.style.code = true;
        inline.runs.push(prove);
        inline.runs.push(docagent_model::Run::text(" twice"));
        section.body.push(Block::Paragraph(inline));
        section.body.push(Block::Paragraph(pre_code_para(
            "docagent prove examples/letter.md",
        )));
        doc.sections.push(section);
        let fonts = FontSet::bundled();
        let list = paint(&layout_document(&doc, &fonts));
        let origin = docagent_layout::PAGE_MARGIN_X_HU;
        let fence = list.ops.iter().find_map(|op| match op {
            Op::Text {
                x,
                size,
                text,
                color,
                ..
            } if text.contains("docagent prove") => Some((*x, *size, *color)),
            _ => None,
        });
        let (fx, fsize, fcolor) = fence.expect("pre code");
        assert_eq!(fsize, docagent_layout::CODE_SPAN_SIZE_HU);
        assert_eq!(fcolor, docagent_layout::CODE_SPAN_COLOR);
        assert_eq!(
            fx,
            origin.saturating_add(docagent_layout::CODE_PAD_X_HU),
            "PDF pre code x {fx} must stay letter CSS 1rem pre pad, not 1rem+0.35em chip inset"
        );
        assert_ne!(
            fx,
            origin
                .saturating_add(docagent_layout::CODE_PAD_X_HU)
                .saturating_add(docagent_layout::CODE_SPAN_PAD_X_HU),
            "inherited code-span 0.35em must not inset the PDF prove fence"
        );
        assert!(docagent_layout::code_span_chip(true, false));
        assert!(!docagent_layout::code_span_chip(true, true));
        assert!(
            list.ops.iter().any(|op| matches!(
                op,
                Op::FillRect { color, w, h, .. }
                    if *color == docagent_layout::CODE_SPAN_BG
                        && *w > 10000
                        && *h >= docagent_layout::CODE_PAD_Y_HU.saturating_mul(2)
            )),
            "PDF pre fill missing: {:?}",
            list.ops
        );
        assert!(
            !list.ops.iter().any(|op| matches!(
                op,
                Op::FillRect { x, w, h, color, .. }
                    if *color == docagent_layout::CODE_SPAN_BG
                        && *x == fx.saturating_sub(docagent_layout::CODE_SPAN_PAD_X_HU)
                        && *h == fsize.saturating_add(
                            docagent_layout::CODE_SPAN_PAD_Y_HU.saturating_mul(2)
                        )
                        && *w > 80
                        && *w < 10000
            )),
            "PDF pre code must not keep an inner mint chip: {:?}",
            list.ops
        );
        assert_eq!(docagent_layout::CODE_PAD_X_HU, 1200);
        assert_eq!(docagent_layout::CODE_SPAN_PAD_X_HU, 350);
        assert_eq!(docagent_layout::CODE_SPAN_SIZE_HU, 920);
        assert_eq!(docagent_layout::MARKER_FG, [19, 78, 74, 255]);
        assert_eq!(docagent_layout::ABBR_UNDERLINE_OFFSET_HU, 150);
        assert_eq!(docagent_layout::HEADING_LEADING_PCT, 120);
        assert_eq!(docagent_layout::H2_RULE_HU, 150);
        assert_eq!(docagent_layout::CELL_BORDER_HU, 75);
        assert_eq!(docagent_layout::PAGE_MARGIN_X_HU, 3300);
        assert_eq!(docagent_layout::PAGE_MARGIN_Y_HU, 3000);
        assert_eq!(docagent_layout::TASK_MARK_ADVANCE_HU, 1380);
        assert_eq!(docagent_layout::H1_LETTER_SPACING_HU, -42);
        assert_eq!(docagent_layout::LIST_NESTED_GAP_HU, 300);
        assert_eq!(docagent_layout::H1_AFTER_HU, 720);
        assert_eq!(docagent_layout::H2_BEFORE_HU, 1320);
        assert_eq!(docagent_layout::H2_AFTER_HU, 540);
        assert_eq!(docagent_layout::BODY_LEADING_PCT, 145);
        assert_eq!(docagent_layout::BODY_FG, [19, 78, 74, 255]);
        assert_eq!(docagent_layout::QUOTE_BAR_W_HU, 300);
        assert_eq!(
            docagent_layout::cell_valign_offset(docagent_model::VAlign::Top, 2000, 5000),
            0
        );
        let pdf = to_pdfa(&list, &fonts).expect("pdf");
        assert!(claims_pdfa(&pdf));
        let mut fenced = Document::new();
        let mut section = Section::default();
        let mut inline = Paragraph::from_text("Run ");
        let mut prove = docagent_model::Run::text("prove");
        prove.style.code = true;
        inline.runs.push(prove);
        inline.runs.push(docagent_model::Run::text(" twice"));
        section.body.push(Block::Paragraph(inline));
        let mut fence = Paragraph::from_text("docagent prove examples/letter.md");
        fence.code_block = true;
        fence.space_before = 80;
        fence.space_after = 200;
        section.body.push(Block::Paragraph(fence));
        fenced.sections.push(section);
        let fenced_pdf = to_pdfa(
            &paint(&layout_document(&fenced, &fonts)),
            &fonts,
        )
        .expect("pdf");
        assert_eq!(
            pdf, fenced_pdf,
            "HTML <pre><code> must paint like a markdown fence: no inherited 0.35em chip"
        );
        let mut plain = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Paragraph(Paragraph::from_text(
            "Run prove twice",
        )));
        section.body.push(Block::Paragraph(Paragraph::from_text(
            "docagent prove examples/letter.md",
        )));
        plain.sections.push(section);
        let plain_pdf = to_pdfa(
            &paint(&layout_document(&plain, &fonts)),
            &fonts,
        )
        .expect("pdf");
        assert_ne!(
            pdf, plain_pdf,
            "pre-code zero pad must still paint the pre mint box vs body type"
        );
    }

    #[test]
    fn letter_inline_code_pdfa_uses_weasyprint_092em() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Run ");
        let mut code = docagent_model::Run::text("prove");
        code.style.code = true;
        p.runs.push(code);
        p.runs.push(docagent_model::Run::text(" twice"));
        section.body.push(Block::Paragraph(p));
        let mut pre = Paragraph::from_text("docagent prove examples/letter.md");
        pre.code_block = true;
        section.body.push(Block::Paragraph(pre));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let run = list.ops.iter().find_map(|op| match op {
            Op::Text { size, text, color, .. } if text.contains("Run") => Some((*size, *color)),
            _ => None,
        });
        let prove = list.ops.iter().find_map(|op| match op {
            Op::Text { size, text, color, .. }
                if text.contains("prove") && *size == docagent_layout::CODE_SPAN_SIZE_HU =>
            {
                Some((*size, *color))
            }
            _ => None,
        });
        let twice = list.ops.iter().find_map(|op| match op {
            Op::Text { size, text, .. } if text.contains("twice") => Some(*size),
            _ => None,
        });
        let pre_size = list.ops.iter().find_map(|op| match op {
            Op::Text { size, text, .. } if text.contains("docagent prove") => Some(*size),
            _ => None,
        });
        let (rsize, rcolor) = run.expect("run");
        let (psize, pcolor) = prove.expect("prove");
        let tsize = twice.expect("twice");
        let csize = pre_size.expect("pre");
        assert_eq!(rsize, docagent_model::DEFAULT_FONT_SIZE_HU);
        assert_eq!(tsize, docagent_model::DEFAULT_FONT_SIZE_HU);
        assert_eq!(
            rcolor,
            docagent_layout::BODY_FG,
            "body beside code must stay letter #134e4a"
        );
        assert_eq!(pcolor, docagent_layout::CODE_SPAN_COLOR);
        assert_eq!(
            psize,
            docagent_layout::CODE_SPAN_SIZE_HU,
            "PDF code size {psize} must be letter CSS 0.92em, not body 10pt"
        );
        assert_eq!(psize, docagent_layout::code_span_size(rsize));
        assert_eq!(
            csize,
            docagent_layout::CODE_SPAN_SIZE_HU,
            "PDF pre code size {csize} must inherit letter CSS 0.92em"
        );
        assert_eq!(docagent_layout::CODE_SPAN_SIZE_HU, 920);
        assert_eq!(docagent_layout::CODE_SPAN_PAD_X_HU, 350);
        assert_eq!(docagent_layout::H2_AFTER_HU, 540);
        assert_eq!(docagent_layout::CELL_PAD_X_HU, 780);
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(claims_pdfa(&pdf));
        let mut plain = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("Run prove twice")));
        section.body.push(Block::Paragraph(Paragraph::from_text(
            "docagent prove examples/letter.md",
        )));
        plain.sections.push(section);
        let plain_pdf = to_pdfa(
            &paint(&layout_document(&plain, &FontSet::bundled())),
            &FontSet::bundled(),
        )
        .expect("pdf");
        assert_ne!(pdf, plain_pdf, "0.92em code type must change PDF bytes");
    }

    #[test]
    fn letter_mark_pdfa_uses_weasyprint_highlight() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("one convert, ");
        let mut mark = docagent_model::Run::text("three hashes");
        mark.style.highlight = Some(docagent_model::Color::MARK);
        p.runs.push(mark);
        p.runs.push(docagent_model::Run::text(", GPU-free"));
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let hashes = list.ops.iter().find_map(|op| match op {
            Op::Text { x, y, size, text, color, .. } if text.contains("hashes") => {
                Some((*x, *y, *size, *color))
            }
            _ => None,
        });
        let after = list.ops.iter().find_map(|op| match op {
            Op::Text { x, text, color, .. } if text.contains("GPU-free") => Some((*x, *color)),
            _ => None,
        });
        let (hx, hy, hsize, hcolor) = hashes.expect("hashes");
        let (ax, acolor) = after.expect("after");
        assert_eq!(
            hcolor,
            docagent_layout::BODY_FG,
            "mark text must be letter body #134e4a, not UA black"
        );
        assert_eq!(acolor, docagent_layout::BODY_FG);
        let fill = list.ops.iter().find_map(|op| match op {
            Op::FillRect { x, y, w, h, color }
                if *color == docagent_layout::MARK_BG && *w > 80 && *h > 80 =>
            {
                Some((*x, *y, *w, *h))
            }
            _ => None,
        });
        let (fx, fy, fw, fh) = fill.expect("mark fill");
        assert_eq!(fx, hx.saturating_sub(docagent_layout::MARK_PAD_X_HU));
        let glyph_w = ax
            .saturating_sub(hx)
            .saturating_sub(docagent_layout::MARK_PAD_X_HU);
        assert_eq!(
            fw,
            glyph_w.saturating_add(docagent_layout::MARK_PAD_X_HU.saturating_mul(2))
        );
        assert_eq!(
            fy,
            hy.saturating_sub(hsize)
                .saturating_sub(docagent_layout::MARK_PAD_Y_HU)
        );
        assert_eq!(
            fh,
            hsize.saturating_add(docagent_layout::MARK_PAD_Y_HU.saturating_mul(2))
        );
        assert!(fh > hsize, "mark fill must be taller than glyphs");
        assert_eq!(docagent_layout::MARK_PAD_X_HU, 200);
        assert_eq!(docagent_layout::CELL_PAD_X_HU, 780);
        assert_eq!(docagent_layout::H2_SIZE_MAX_HU, 1380);
        assert_eq!(docagent_layout::HR_BEFORE_HU, 1320);
        assert_eq!(docagent_layout::LINK_UNDERLINE_HU, 80);
        assert_eq!(docagent_layout::QUOTE_PAD_X_HU, 1200);
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(claims_pdfa(&pdf));
        let mut plain = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Paragraph(Paragraph::from_text(
            "one convert, three hashes, GPU-free",
        )));
        plain.sections.push(section);
        let plain_pdf = to_pdfa(
            &paint(&layout_document(&plain, &FontSet::bundled())),
            &FontSet::bundled(),
        )
        .expect("pdf");
        assert_ne!(pdf, plain_pdf, "mark highlight must change PDF bytes");
    }

    #[test]
    fn code_span_paints_non_grid_fill() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("prove");
        p.runs[0].style.code = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, h, w, .. }
                        if *color == [204, 251, 241, 255] && *w > 80 && *h > 80
                )
            }),
            "code span must paint a background fill"
        );
        let coded = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        let mut plain = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("prove")));
        plain.sections.push(section);
        let plain_pdf = to_pdfa(
            &paint(&layout_document(&plain, &FontSet::bundled())),
            &FontSet::bundled(),
        )
        .expect("pdf");
        assert_ne!(coded, plain_pdf, "code span must change PDF bytes");
        assert!(claims_pdfa(&coded));
    }

    #[test]
    fn quote_bar_paints_non_grid_fill() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        p.quote = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Text { text, color, .. }
                        if text.contains("Replay is evidence")
                            && *color == docagent_layout::QUOTE_FG
                )
            }),
            "blockquote text must be letter CSS #134e4a"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, h, w, .. }
                        if *color == docagent_layout::QUOTE_FG
                            && *w == docagent_layout::QUOTE_BAR_W_HU
                            && *h > 200
                )
            }),
            "quote bar must paint a tall fill"
        );
        assert_eq!(docagent_layout::QUOTE_FG, [19, 78, 74, 255]);
        assert_eq!(docagent_layout::QUOTE_BAR_W_HU, 300);
        let quoted = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        let mut plain = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Paragraph(Paragraph::from_text(
            "Replay is evidence. Same fonts, same bytes.",
        )));
        plain.sections.push(section);
        let plain_pdf = to_pdfa(
            &paint(&layout_document(&plain, &FontSet::bundled())),
            &FontSet::bundled(),
        )
        .expect("pdf");
        assert_ne!(quoted, plain_pdf, "quote bar must change PDF bytes");
        assert!(claims_pdfa(&quoted));
    }

    #[test]
    fn strikethrough_paints_non_grid_fill() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("guess");
        p.runs[0].style.strike = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, h, w, .. }
                        if *color == docagent_layout::STRIKE_COLOR
                            && *h == docagent_layout::STRIKE_HU
                            && *w > 80
                )
            }),
            "strikethrough must paint a fill"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Text { text, color, .. }
                        if text.contains("guess") && *color == docagent_layout::STRIKE_COLOR
                )
            }),
            "s glyphs must be letter CSS #134e4a"
        );
        let struck = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        let mut plain = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("guess")));
        plain.sections.push(section);
        let plain_pdf = to_pdfa(
            &paint(&layout_document(&plain, &FontSet::bundled())),
            &FontSet::bundled(),
        )
        .expect("pdf");
        assert_ne!(struck, plain_pdf, "strikethrough must change PDF bytes");
        assert!(claims_pdfa(&struck));
    }

    #[test]
    fn letter_strike_and_underline_pdfa_uses_weasyprint_s_u() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Agents ");
        let mut strike = docagent_model::Run::text("guess");
        strike.style.strike = true;
        p.runs.push(strike);
        p.runs.push(docagent_model::Run::text("."));
        section.body.push(Block::Paragraph(p));
        let mut under = Paragraph::from_text("09:00 local");
        under.runs[0].style.underline = docagent_model::Underline::Single;
        section.body.push(Block::Paragraph(under));
        let mut table = docagent_model::Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        section
            .body
            .push(Block::Paragraph(strong_em_para()));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        section
            .body
            .push(Block::Paragraph(task_para("Receiving dock is clear", true)));
        let mut code = docagent_model::Run::text("prove");
        code.style.code = true;
        let mut code_p = Paragraph::from_text("Run ");
        code_p.runs.push(code);
        section.body.push(Block::Paragraph(code_p));
        let mut mark = Paragraph::from_text("three hashes");
        mark.runs[0].style.highlight = Some(docagent_model::Color::MARK);
        section.body.push(Block::Paragraph(mark));
        section
            .body
            .push(Block::Break(docagent_model::BreakKind::Thematic));
        let mut link = Paragraph::from_text("");
        link.runs = vec![docagent_model::Run::hyperlink(
            "DocAgent",
            "https://github.com/kevin9327/docagent",
        )];
        section.body.push(Block::Paragraph(link));
        let mut pdfa = Paragraph::from_text("PDF/");
        let mut sup = docagent_model::Run::text("A");
        sup.style.superscript = true;
        pdfa.runs.push(sup);
        section.body.push(Block::Paragraph(pdfa));
        let mut h2o = Paragraph::from_text("H");
        let mut sub = docagent_model::Run::text("2");
        sub.style.subscript = true;
        h2o.runs.push(sub);
        h2o.runs.push(docagent_model::Run::text("O"));
        section.body.push(Block::Paragraph(h2o));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let guess = list.ops.iter().find_map(|op| match op {
            Op::Text { x, y, size, text, color, .. } if text.contains("guess") => {
                Some((*x, *y, *size, *color))
            }
            _ => None,
        });
        let (gx, gy, gsize, gcolor) = guess.expect("strike text");
        assert_eq!(gcolor, docagent_layout::STRIKE_COLOR);
        let guess_i = list
            .ops
            .iter()
            .position(|op| matches!(op, Op::Text { text, .. } if text.contains("guess")))
            .expect("strike glyphs");
        let strike_i = list
            .ops
            .iter()
            .position(|op| {
                matches!(
                    op,
                    Op::FillRect { x, y, h, color, .. }
                        if *color == docagent_layout::STRIKE_COLOR
                            && *h == docagent_layout::STRIKE_HU
                            && *x == gx
                            && *y == gy.saturating_sub(gsize / 3)
                )
            })
            .expect("strike fill");
        assert!(
            strike_i > guess_i,
            "strikethrough must paint after glyphs like WeasyPrint, not missing under the ink"
        );
        let local = list.ops.iter().find_map(|op| match op {
            Op::Text { x, y, text, color, .. } if text.contains("09:00") => {
                Some((*x, *y, *color))
            }
            _ => None,
        });
        let (ux, uy, ucolor) = local.expect("u text");
        assert_eq!(
            ucolor,
            docagent_layout::BODY_FG,
            "u text must be letter body #134e4a, not UA black"
        );
        let under_i = list
            .ops
            .iter()
            .position(|op| {
                matches!(
                    op,
                    Op::FillRect { x, y, h, color, .. }
                        if *color == docagent_layout::STRIKE_COLOR
                            && *h == docagent_layout::UNDERLINE_HU
                            && *x == ux
                            && *y == uy.saturating_add(docagent_layout::UNDERLINE_OFFSET_HU)
                )
            })
            .expect("u underline fill");
        let local_i = list
            .ops
            .iter()
            .position(|op| matches!(op, Op::Text { text, .. } if text.contains("09:00")))
            .expect("u glyphs");
        assert!(
            under_i > local_i,
            "underline must paint after glyphs like WeasyPrint, not missing under the ink"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, w, h, .. }
                        if *color == docagent_layout::TH_BG && *w > 80 && *h > 80
                )
            }),
            "th fill must not regress"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Text { text, color, bold: true, italic: false, .. }
                        if text.contains("GPU-free") && *color == docagent_layout::BODY_FG
                )
            }),
            "strong must not regress from letter body #134e4a"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Text { text, color, italic: true, bold: false, .. }
                        if text.contains("byte-for-byte") && *color == docagent_layout::BODY_FG
                )
            }),
            "em must not regress from letter body #134e4a"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Text { text, color, .. }
                        if text.contains("Replay is evidence")
                            && *color == docagent_layout::QUOTE_FG
                )
            }),
            "blockquote text must be letter CSS #134e4a"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, w, h, .. }
                        if *color == docagent_layout::QUOTE_FG
                            && *w == docagent_layout::QUOTE_BAR_W_HU
                            && *h > 200
                )
            }),
            "quote bar must not regress"
        );
        let box_hu = docagent_layout::TASK_BOX_HU;
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, w, h, .. }
                        if *color == [19, 78, 74, 255] && *w == box_hu && *h == box_hu
                )
            }),
            "task box must not regress"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, .. } if *color == docagent_layout::CODE_SPAN_BG
                )
            }),
            "code fill must not regress"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, .. } if *color == docagent_layout::MARK_BG
                )
            }),
            "mark fill must not regress"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, h, w, .. }
                        if *color == [19, 78, 74, 255]
                            && *h == docagent_layout::HR_THICK_HU
                            && *w > 10000
                )
            }),
            "hr must not regress"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, h, .. }
                        if *color == docagent_layout::LINK_COLOR
                            && *h == docagent_layout::LINK_UNDERLINE_HU
                )
            }),
            "link underline must not regress"
        );
        assert_eq!(docagent_layout::STRIKE_COLOR, [19, 78, 74, 255]);
        assert_eq!(docagent_layout::QUOTE_FG, [19, 78, 74, 255]);
        assert_eq!(docagent_layout::BODY_FG, [19, 78, 74, 255]);
        assert_ne!(
            docagent_layout::BODY_FG,
            [0, 0, 0, 255],
            "body must not regress to UA black"
        );
        assert_eq!(docagent_layout::QUOTE_BAR_W_HU, 300);
        assert_eq!(docagent_layout::STRIKE_HU, 80);
        assert_eq!(docagent_layout::UNDERLINE_HU, 80);
        assert_eq!(docagent_layout::UNDERLINE_OFFSET_HU, 100);
        let pdf_run = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, .. } if text.contains("PDF") => Some((*y, *size)),
            _ => None,
        });
        let raised = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, .. } if text == "A" => Some((*y, *size)),
            _ => None,
        });
        let (py, psize) = pdf_run.expect("PDF");
        let (ay, asize) = raised.expect("super");
        assert_eq!(asize, docagent_layout::SUPER_SUB_SIZE_HU);
        assert_eq!(asize, docagent_layout::super_sub_size(psize));
        assert_eq!(ay, py.saturating_sub(docagent_layout::SUPER_SUB_SHIFT_HU));
        let h = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, .. } if text == "H" => Some((*y, *size)),
            _ => None,
        });
        let lowered = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, .. } if text == "2" => Some((*y, *size)),
            _ => None,
        });
        let (hy, hsize) = h.expect("H");
        let (ty, tsize) = lowered.expect("sub");
        assert_eq!(tsize, docagent_layout::SUPER_SUB_SIZE_HU);
        assert_eq!(tsize, docagent_layout::super_sub_size(hsize));
        assert_eq!(ty, hy.saturating_add(docagent_layout::SUPER_SUB_SHIFT_HU));
        assert_eq!(docagent_layout::SUPER_SUB_SIZE_HU, 750);
        assert_eq!(docagent_layout::SUPER_SUB_SHIFT_HU, 375);
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(claims_pdfa(&pdf));
        let mut plain = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("Agents guess.")));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("09:00 local")));
        plain.sections.push(section);
        let plain_pdf = to_pdfa(
            &paint(&layout_document(&plain, &FontSet::bundled())),
            &FontSet::bundled(),
        )
        .expect("pdf");
        assert_ne!(
            pdf, plain_pdf,
            "strike/underline must change PDF bytes vs missing decorations"
        );
    }

    #[test]
    fn synthetic_bold_changes_pdf_bytes() {
        fn pdf_for(bold: bool) -> Vec<u8> {
            let mut doc = Document::new();
            let mut section = Section::default();
            let mut p = Paragraph::from_text("GPU-free");
            p.runs[0].style.bold = bold;
            section.body.push(Block::Paragraph(p));
            doc.sections.push(section);
            let tree = layout_document(&doc, &FontSet::bundled());
            let list = paint(&tree);
            assert!(list.ops.iter().any(|op| {
                matches!(op, Op::Text { text, bold: b, .. } if text.contains("GPU-free") && *b == bold)
            }));
            to_pdfa(&list, &FontSet::bundled()).expect("pdf")
        }
        let plain = pdf_for(false);
        let heavy = pdf_for(true);
        assert_ne!(plain, heavy, "synthetic bold must change PDF bytes");
        assert!(heavy.len() >= plain.len());
        assert_eq!(
            super::BOLD_STROKE_DIV as i32,
            24,
            "WeasyPrint/Pango fake-bold is em/24"
        );
        assert!(claims_pdfa(&heavy));
    }

    #[test]
    fn synthetic_italic_changes_pdf_bytes() {
        fn pdf_for(italic: bool) -> Vec<u8> {
            let mut doc = Document::new();
            let mut section = Section::default();
            let mut p = Paragraph::from_text("byte-for-byte");
            p.runs[0].style.italic = italic;
            section.body.push(Block::Paragraph(p));
            doc.sections.push(section);
            let tree = layout_document(&doc, &FontSet::bundled());
            let list = paint(&tree);
            assert!(list.ops.iter().any(|op| {
                matches!(op, Op::Text { text, italic: i, .. } if text.contains("byte-for-byte") && *i == italic)
            }));
            to_pdfa(&list, &FontSet::bundled()).expect("pdf")
        }
        let roman = pdf_for(false);
        let oblique = pdf_for(true);
        assert_ne!(roman, oblique, "synthetic italic must change PDF bytes");
        let want = 12f32.to_radians().tan();
        assert!(
            (super::ITALIC_SHEAR - want).abs() < 1e-5,
            "em shear {} must stay WeasyPrint 12°, not a missing slant",
            super::ITALIC_SHEAR
        );
        assert!(claims_pdfa(&oblique));
    }

    const MARK_PNG: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/assets/mark.png"
    ));

    fn png_ihdr_px(bytes: &[u8]) -> (u32, u32) {
        const SIG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        assert!(
            bytes.len() >= 24 && bytes.starts_with(SIG) && &bytes[12..16] == b"IHDR",
            "mark.png missing IHDR"
        );
        let w = u32::from_be_bytes(bytes[16..20].try_into().expect("ihdr width"));
        let h = u32::from_be_bytes(bytes[20..24].try_into().expect("ihdr height"));
        assert!(w > 0 && h > 0, "empty PNG");
        (w, h)
    }

    fn px_to_hu_96(px: u32) -> i32 {
        i32::try_from(i64::from(px).saturating_mul(i64::from(docagent_model::HU_PER_INCH)) / 96)
            .unwrap_or(i32::MAX)
    }

    fn mark_png_hu() -> (i32, i32) {
        let (w, h) = png_ihdr_px(MARK_PNG);
        (px_to_hu_96(w), px_to_hu_96(h))
    }

    fn mark_run(width: i32, height: i32) -> docagent_model::Run {
        docagent_model::Run {
            style: docagent_model::CharStyle::default(),
            content: docagent_model::RunContent::Inline(docagent_model::InlineObject::Image(
                docagent_model::ImageData {
                    bytes: MARK_PNG.to_vec(),
                    mime: "image/png".into(),
                    width,
                    height,
                    alt_text: Some("mark".into()),
                    wrap: docagent_model::WrapMode::Inline,
                },
            )),
        }
    }

    fn doc_with_mark() -> Document {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![mark_run(1600, 1600)];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        doc
    }

    fn letter_with_mark(width: i32, height: i32) -> Document {
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
    fn png_image_embeds_in_pdfa() {
        let list = paint(&layout_document(&doc_with_mark(), &FontSet::bundled()));
        let (ew, eh) = docagent_layout::min_on_page_size(1600, 1600);
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Image { w, h, bytes, mime, .. }
                        if *w == ew && *h == eh && bytes.as_slice() == MARK_PNG && mime == "image/png"
                )
            }),
            "paint must forward PNG bytes"
        );
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(pdf.starts_with(b"%PDF"));
        assert!(claims_pdfa(&pdf), "pdfa identifier missing");
        let s = String::from_utf8_lossy(&pdf);
        assert!(
            s.contains("/Image"),
            "PDF image XObject missing: {}",
            s.chars().take(400).collect::<String>()
        );
        assert!(s.contains("/Width"), "image width missing");
        assert!(s.contains("/Height"), "image height missing");
        let mut blank = Document::new();
        blank.sections.push(Section::default());
        let blank_pdf = to_pdfa(
            &paint(&layout_document(&blank, &FontSet::bundled())),
            &FontSet::bundled(),
        )
        .expect("pdf");
        assert!(
            pdf.len() > blank_pdf.len(),
            "embedded PNG must enlarge PDF ({} vs {})",
            pdf.len(),
            blank_pdf.len()
        );
    }

    #[test]
    fn letter_mark_png_pdfa_uses_minimum_on_page_size() {
        assert_eq!(png_ihdr_px(MARK_PNG), (16, 16), "fixture is 16×16 px");
        let (w, h) = mark_png_hu();
        assert_eq!((w, h), (px_to_hu_96(16), px_to_hu_96(16)));
        assert!(w < docagent_layout::MIN_IMAGE_EDGE_HU);
        let (ew, eh) = docagent_layout::min_on_page_size(w, h);
        assert!(ew >= docagent_layout::MIN_IMAGE_EDGE_HU);
        assert!(eh >= docagent_layout::MIN_IMAGE_EDGE_HU);
        assert!(ew >= docagent_model::HU_PER_INCH / 4);
        let list = paint(&layout_document(
            &letter_with_mark(w, h),
            &FontSet::bundled(),
        ));
        let image = list.ops.iter().find_map(|op| match op {
            Op::Image { x, y, w, h, bytes, mime }
                if bytes.as_slice() == MARK_PNG && mime == "image/png" =>
            {
                Some((*x, *y, *w, *h))
            }
            _ => None,
        });
        let (ix, iy, iw, ih) = image.expect("letter image op missing");
        assert_eq!(
            (iw, ih),
            (ew, eh),
            "PDF/A paint must use the 24pt on-page floor, not 16px at 96dpi"
        );
        let body = list.ops.iter().find_map(|op| match op {
            Op::Text { x, y, size, text, .. } if text.contains("shipment") => {
                Some((*x, *y, *size))
            }
            _ => None,
        });
        let (bx, by, bsize) = body.expect("body text op missing");
        assert!(
            by.saturating_sub(bsize) >= iy.saturating_add(ih),
            "body baseline {by} size {bsize} overlaps image ({ix},{iy}) {iw}×{ih}; body x {bx}"
        );
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(pdf.starts_with(b"%PDF"));
        assert!(claims_pdfa(&pdf), "pdfa identifier missing");
        let s = String::from_utf8_lossy(&pdf);
        assert!(
            has_image_xobject(&s),
            "PDF image XObject missing: {}",
            s.chars().take(400).collect::<String>()
        );
        assert!(
            s.contains("/Width 16") && s.contains("/Height 16"),
            "XObject must keep mark.png pixel size, not ImageData HU"
        );
        let mut plain = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("Northwind Freight")));
        section.body.push(Block::Paragraph(Paragraph::from_text(
            "The shipment left Busan on 4 September",
        )));
        plain.sections.push(section);
        let without = to_pdfa(
            &paint(&layout_document(&plain, &FontSet::bundled())),
            &FontSet::bundled(),
        )
        .expect("pdf");
        let without_s = String::from_utf8_lossy(&without);
        assert!(
            !has_image_xobject(&without_s),
            "plain letter must not embed an image XObject"
        );
        assert!(
            pdf.len() > without.len(),
            "letter PNG must enlarge PDF ({} vs {})",
            pdf.len(),
            without.len()
        );
    }

    #[test]
    fn letter_page_pdfa_keeps_dense_type_table_and_mark() {
        let (w, h) = mark_png_hu();
        let (ew, eh) = docagent_layout::min_on_page_size(w, h);
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.size = 1800;
        h1.runs[0].style.bold = true;
        h1.space_after = 400;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation");
        h2.outline_level = Some(2);
        h2.runs[0].style.size = 1400;
        h2.runs[0].style.bold = true;
        h2.space_before = 360;
        h2.space_after = 240;
        section.body.push(Block::Paragraph(h2));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![mark_run(w, h)];
        section.body.push(Block::Paragraph(img_p));
        let mut body = Paragraph::from_text("The shipment left Busan on 4 September");
        body.space_after = 200;
        section.body.push(Block::Paragraph(body));
        let mut table = docagent_model::Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let fonts = FontSet::bundled();
        let list = paint(&layout_document(&doc, &fonts));
        let h1_size = list.ops.iter().find_map(|op| match op {
            Op::Text { size, text, color, .. } if text.contains("Northwind") => {
                Some((*size, *color))
            }
            _ => None,
        });
        let h2_size = list.ops.iter().find_map(|op| match op {
            Op::Text { size, text, color, .. } if text.contains("Delivery") => {
                Some((*size, *color))
            }
            _ => None,
        });
        let (h1_size, h1_color) = h1_size.expect("h1");
        let (h2_size, h2_color) = h2_size.expect("h2");
        assert_eq!(
            h1_size,
            docagent_layout::H1_SIZE_MAX_HU,
            "h1 size {h1_size} must be letter CSS 1.75rem"
        );
        assert_eq!(
            h1_color,
            docagent_layout::BODY_FG,
            "h1 must paint letter CSS #134e4a, not UA black"
        );
        assert_eq!(
            h2_size,
            docagent_layout::H2_SIZE_MAX_HU,
            "h2 size {h2_size} must be letter CSS 1.15rem"
        );
        assert_eq!(
            h2_color,
            docagent_layout::BODY_FG,
            "h2 must paint letter CSS #134e4a, not UA black"
        );
        assert_eq!(docagent_layout::H1_RULE_HU, 225);
        assert_eq!(docagent_layout::H1_RULE_GAP_HU, 480);
        let image = list.ops.iter().find_map(|op| match op {
            Op::Image { x, y, w, h, bytes, mime }
                if bytes.as_slice() == MARK_PNG && mime == "image/png" =>
            {
                Some((*x, *y, *w, *h))
            }
            _ => None,
        });
        let (_ix, iy, iw, ih) = image.expect("mark");
        assert_eq!((iw, ih), (ew, eh));
        let body = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, .. } if text.contains("shipment") => Some((*y, *size)),
            _ => None,
        });
        let (by, bsize) = body.expect("body");
        assert!(
            by.saturating_sub(bsize) >= iy.saturating_add(ih),
            "body overlaps mark at y {iy}"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, h, w, .. }
                        if *color == docagent_layout::BODY_FG
                            && *h == docagent_layout::H1_RULE_HU
                            && *w > 1000
                )
            }),
            "h1 3px #134e4a missing"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, w, h, .. }
                        if *color == docagent_layout::TH_BG && *w > 80 && *h > 80
                )
            }),
            "table header fill missing"
        );
        let pdf = to_pdfa(&list, &fonts).expect("pdf");
        assert!(pdf.starts_with(b"%PDF"));
        assert!(claims_pdfa(&pdf));
        let s = String::from_utf8_lossy(&pdf);
        assert!(
            has_image_xobject(&s),
            "letter PNG XObject missing"
        );
    }

    #[test]
    fn letter_table_cell_padding_is_weasyprint_letter_css() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = docagent_model::Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let hop_line = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.text.contains("Hop"))
            .expect("header line");
        let list = paint(&tree);
        let cell = list.ops.iter().find_map(|op| match op {
            Op::FillRect { x, y, w, h, color }
                if *color == docagent_layout::TH_BG && *w > 80 && *h > 80 =>
            {
                Some((*x, *y, *w, *h))
            }
            _ => None,
        });
        let (cx, cy, _, ch) = cell.expect("header fill");
        let hop = list.ops.iter().find_map(|op| match op {
            Op::Text { x, y, size, text, .. } if text.contains("Hop") => Some((*x, *y, *size)),
            _ => None,
        });
        let (tx, ty, tsize) = hop.expect("header text");
        assert_eq!(
            tx,
            cx.saturating_add(docagent_layout::CELL_PAD_X_HU),
            "PDF cell pad x {} must be letter CSS 0.65rem",
            tx.saturating_sub(cx)
        );
        assert_eq!(
            hop_line.y,
            cy.saturating_add(docagent_layout::CELL_PAD_Y_HU),
            "PDF header line box {} must sit on letter CSS 0.4rem pad",
            hop_line.y.saturating_sub(cy)
        );
        assert_eq!(
            ty,
            hop_line.y.saturating_add(hop_line.baseline),
            "PDF th baseline {ty} must keep the 0.4rem cell pad under the type"
        );
        let glyph_top = ty.saturating_sub(tsize);
        assert!(
            glyph_top >= cy.saturating_add(docagent_layout::CELL_PAD_Y_HU),
            "PDF cell pad y {} must be letter CSS 0.4rem",
            glyph_top.saturating_sub(cy)
        );
        assert!(
            ch > docagent_layout::CELL_PAD_Y_HU.saturating_mul(2),
            "header row {ch} must include vertical cell pad"
        );
        assert_eq!(
            tsize,
            docagent_layout::CELL_SIZE_HU,
            "PDF th type {tsize} must be letter CSS 0.95rem, not body 10pt"
        );
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(claims_pdfa(&pdf));
    }

    #[test]
    fn letter_table_grid_pdfa_is_weasyprint_collapse() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = docagent_model::Table::from_cells(vec![
            vec!["Hop".into(), "File".into(), "Proof".into()],
            vec!["Input".into(), "letter.md".into(), "input SHA-256".into()],
            vec!["Plan".into(), "layout i32".into(), "plan SHA-256".into()],
        ]);
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let headers: Vec<(i32, i32, i32, i32)> = list
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::FillRect { x, y, w, h, color }
                    if *color == docagent_layout::TH_BG && *w > 80 && *h > 80 =>
                {
                    Some((*x, *y, *w, *h))
                }
                _ => None,
            })
            .collect();
        assert_eq!(headers.len(), 3, "hop header fills missing");
        let (hx, hy, hw, hh) = headers[0];
        let (fx, ..) = headers[1];
        assert_eq!(fx, hx.saturating_add(hw), "header cells must share an edge");
        let v_edge = fx;
        let mut v_xs: Vec<i32> = list
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::FillRect { x, w, h, color, .. }
                    if *color == docagent_layout::CELL_BORDER
                        && *w == docagent_layout::CELL_BORDER_HU
                        && *h > docagent_layout::CELL_BORDER_HU
                        && (*x == v_edge
                            || *x == v_edge.saturating_sub(docagent_layout::CELL_BORDER_HU)) =>
                {
                    Some(*x)
                }
                _ => None,
            })
            .collect();
        v_xs.sort_unstable();
        v_xs.dedup();
        assert_eq!(
            v_xs,
            [v_edge],
            "PDF internal vertical must collapse to one 1px, not two adjacent 1px boxes"
        );
        let seam = hy.saturating_add(hh);
        let gray_at_header_seam = list.ops.iter().any(|op| {
            matches!(
                op,
                Op::FillRect { y, w, h, color, .. }
                    if *color == docagent_layout::CELL_BORDER
                        && *h == docagent_layout::CELL_BORDER_HU
                        && *w > docagent_layout::CELL_BORDER_HU
                        && (*y == seam
                            || *y == seam.saturating_sub(docagent_layout::CELL_BORDER_HU))
            )
        });
        assert!(
            !gray_at_header_seam,
            "PDF header/body seam must stay 2px #0d9488, not a stacked 1px gray"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { y, h, color, .. }
                        if *color == docagent_layout::TH_RULE
                            && *h == docagent_layout::TH_RULE_HU
                            && *y == seam.saturating_sub(docagent_layout::TH_RULE_HU)
                )
            }),
            "th 2px #0d9488 must sit on the collapsed header seam"
        );
        let bodies: Vec<(i32, i32, i32, i32)> = list
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::FillRect { x, y, w, h, color }
                    if *color == [255, 255, 255, 255] && *w > 80 && *h > 80 && *y > hy =>
                {
                    Some((*x, *y, *w, *h))
                }
                _ => None,
            })
            .collect();
        let input = bodies
            .iter()
            .find(|(x, ..)| *x == hx)
            .expect("input cell");
        let plan = bodies
            .iter()
            .find(|(x, y, ..)| *x == hx && *y > input.1)
            .expect("plan cell");
        let body_seam = input.1.saturating_add(input.3);
        assert_eq!(plan.1, body_seam);
        let mut h_ys: Vec<i32> = list
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::FillRect { y, w, h, color, .. }
                    if *color == docagent_layout::CELL_BORDER
                        && *h == docagent_layout::CELL_BORDER_HU
                        && *w > docagent_layout::CELL_BORDER_HU
                        && (*y == body_seam
                            || *y
                                == body_seam
                                    .saturating_sub(docagent_layout::CELL_BORDER_HU)) =>
                {
                    Some(*y)
                }
                _ => None,
            })
            .collect();
        h_ys.sort_unstable();
        h_ys.dedup();
        assert_eq!(
            h_ys,
            [body_seam],
            "PDF internal horizontal must collapse to one 1px, not two stacked 1px boxes"
        );
        assert_eq!(docagent_layout::CELL_BORDER_HU, 75);
        assert_eq!(
            docagent_layout::TH_RULE_HU,
            2 * docagent_layout::CELL_BORDER_HU
        );
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(claims_pdfa(&pdf));
    }

    #[test]
    fn letter_paragraph_gap_after_mark_is_weasyprint_letter_css() {
        let (w, h) = mark_png_hu();
        let (ew, eh) = docagent_layout::min_on_page_size(w, h);
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![mark_run(w, h)];
        img_p.space_after = 200;
        section.body.push(Block::Paragraph(img_p));
        let mut body = Paragraph::from_text("6 September 2026");
        body.space_after = 200;
        section.body.push(Block::Paragraph(body));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let date_line = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.text.contains("September"))
            .expect("date line");
        let img_line = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.image.is_some())
            .expect("mark line");
        assert_eq!(
            date_line.y,
            img_line
                .y
                .saturating_add(eh)
                .saturating_add(docagent_layout::IMAGE_AFTER_HU),
            "PDF line box after mark must be letter CSS 0.7rem"
        );
        let list = paint(&tree);
        let image = list.ops.iter().find_map(|op| match op {
            Op::Image { x, y, w, h, bytes, mime }
                if bytes.as_slice() == MARK_PNG && mime == "image/png" =>
            {
                Some((*x, *y, *w, *h))
            }
            _ => None,
        });
        let (_ix, iy, iw, ih) = image.expect("mark");
        assert_eq!((iw, ih), (ew, eh));
        let date = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, .. } if text.contains("September") => Some((*y, *size)),
            _ => None,
        });
        let (by, bsize) = date.expect("date");
        assert_eq!(
            bsize,
            docagent_model::DEFAULT_FONT_SIZE_HU,
            "PDF type after the 24pt mark must stay letter CSS 10pt body"
        );
        assert_eq!(
            by,
            date_line.y.saturating_add(date_line.baseline),
            "PDF date baseline must keep the 0.7rem gap under the type"
        );
        let glyph_top = by.saturating_sub(bsize);
        assert!(
            glyph_top >= iy.saturating_add(ih),
            "date overlaps mark"
        );
        let gap = glyph_top.saturating_sub(iy.saturating_add(ih));
        assert!(
            gap >= docagent_layout::IMAGE_AFTER_HU,
            "gap after mark {gap} must be letter CSS 0.7rem"
        );
        assert!(
            gap < docagent_layout::IMAGE_AFTER_HU.saturating_add(400),
            "gap after mark {gap} must not keep body space_after 200"
        );
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(claims_pdfa(&pdf));
        assert!(has_image_xobject(&String::from_utf8_lossy(&pdf)));
    }

    #[test]
    fn letter_gap_before_mark_under_h2_is_weasyprint_letter_css() {
        let (w, h) = mark_png_hu();
        let (ew, eh) = docagent_layout::min_on_page_size(w, h);
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h2 = Paragraph::from_text("Delivery confirmation");
        h2.outline_level = Some(2);
        h2.runs[0].style.size = 1400;
        h2.runs[0].style.bold = true;
        h2.space_after = 240;
        section.body.push(Block::Paragraph(h2));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![mark_run(w, h)];
        img_p.space_after = 200;
        section.body.push(Block::Paragraph(img_p));
        let mut date = Paragraph::from_text("6 September 2026");
        date.space_after = 200;
        section.body.push(Block::Paragraph(date));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let h2 = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, color, .. } if text.contains("Delivery") => {
                Some((*y, *size, *color))
            }
            _ => None,
        });
        let (_h2y, h2size, h2color) = h2.expect("h2");
        assert_eq!(h2size, docagent_layout::H2_SIZE_MAX_HU);
        assert_eq!(h2color, docagent_layout::BODY_FG);
        let rule = list.ops.iter().find_map(|op| match op {
            Op::FillRect { y, h, color, w, .. }
                if *color == [13, 148, 136, 255]
                    && *h == docagent_layout::H2_RULE_HU
                    && *w > 80 =>
            {
                Some(*y)
            }
            _ => None,
        });
        let ry = rule.expect("h2 2px rule");
        let image = list.ops.iter().find_map(|op| match op {
            Op::Image { y, w, h, bytes, mime, .. }
                if bytes.as_slice() == MARK_PNG && mime == "image/png" =>
            {
                Some((*y, *w, *h))
            }
            _ => None,
        });
        let (iy, iw, ih) = image.expect("mark");
        assert_eq!((iw, ih), (ew, eh));
        let after_rule = iy.saturating_sub(ry.saturating_add(docagent_layout::H2_RULE_HU));
        assert!(
            after_rule >= docagent_layout::IMAGE_BEFORE_HU,
            "PDF gap after h2 rule {after_rule} must be letter CSS 0.6rem, not jammed under H2"
        );
        let date = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, color, .. } if text.contains("September") => {
                Some((*y, *size, *color))
            }
            _ => None,
        });
        let (by, bsize, bcolor) = date.expect("date");
        assert_eq!(bcolor, docagent_layout::BODY_FG);
        let glyph_top = by.saturating_sub(bsize);
        let gap = glyph_top.saturating_sub(iy.saturating_add(ih));
        assert!(
            gap >= docagent_layout::IMAGE_AFTER_HU,
            "PDF gap after mark {gap} must stay letter CSS 0.7rem"
        );
        assert_eq!(docagent_layout::IMAGE_BEFORE_HU, 720);
        assert_eq!(docagent_layout::H2_RULE_HU, 150);
        assert_eq!(docagent_layout::H2_AFTER_HU, 540);
        assert_eq!(docagent_layout::CELL_SIZE_HU, 1140);
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(claims_pdfa(&pdf));
        assert!(has_image_xobject(&String::from_utf8_lossy(&pdf)));
    }

    #[test]
    fn letter_first_line_after_h2_is_weasyprint_045rem() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h2 = Paragraph::from_text("Delivery confirmation");
        h2.outline_level = Some(2);
        h2.runs[0].style.size = 1400;
        h2.runs[0].style.bold = true;
        h2.space_after = 240;
        section.body.push(Block::Paragraph(h2));
        let mut date = Paragraph::from_text("6 September 2026");
        date.space_after = 200;
        section.body.push(Block::Paragraph(date));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let h2 = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, color, .. } if text.contains("Delivery") => {
                Some((*y, *size, *color))
            }
            _ => None,
        });
        let (_h2y, h2size, h2color) = h2.expect("h2");
        assert_eq!(h2size, docagent_layout::H2_SIZE_MAX_HU);
        assert_eq!(h2color, docagent_layout::BODY_FG);
        let rule = list.ops.iter().find_map(|op| match op {
            Op::FillRect { y, h, color, w, .. }
                if *color == [13, 148, 136, 255]
                    && *h == docagent_layout::H2_RULE_HU
                    && *w > 80 =>
            {
                Some(*y)
            }
            _ => None,
        });
        let ry = rule.expect("h2 2px rule");
        let date = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, color, .. } if text.contains("September") => {
                Some((*y, *size, *color))
            }
            _ => None,
        });
        let (by, bsize, bcolor) = date.expect("date");
        assert_eq!(bcolor, docagent_layout::BODY_FG);
        let glyph_top = by.saturating_sub(bsize);
        assert!(
            glyph_top >= ry.saturating_add(docagent_layout::H2_RULE_HU),
            "first line after h2 overlaps the 2px rule"
        );
        let after_rule = glyph_top.saturating_sub(ry.saturating_add(docagent_layout::H2_RULE_HU));
        assert!(
            after_rule >= docagent_layout::H2_AFTER_HU,
            "PDF first line after h2 {after_rule} must be letter CSS 0.45rem, not IR 240"
        );
        assert!(
            after_rule < docagent_layout::H2_AFTER_HU.saturating_add(400),
            "body after h2 {after_rule} must stay 0.45rem, not the 0.6rem 24pt-mark gap"
        );
        assert_eq!(docagent_layout::H2_AFTER_HU, 540);
        assert_eq!(docagent_layout::H2_BEFORE_HU, 1320);
        assert_eq!(docagent_layout::H1_AFTER_HU, 720);
        assert_eq!(docagent_layout::H1_RULE_HU, 225);
        assert_eq!(docagent_layout::H1_RULE_GAP_HU, 480);
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(claims_pdfa(&pdf));
    }

    #[test]
    fn letter_body_type_is_weasyprint_heading_size_and_paragraph_gap() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.size = 1800;
        section.body.push(Block::Paragraph(h1));
        let mut date = Paragraph::from_text("6 September 2026");
        date.space_after = 200;
        section.body.push(Block::Paragraph(date));
        let mut dest = Paragraph::from_text("To: operations desk");
        dest.space_after = 200;
        section.body.push(Block::Paragraph(dest));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let h1_size = list.ops.iter().find_map(|op| match op {
            Op::Text { size, text, .. } if text.contains("Northwind") => Some(*size),
            _ => None,
        });
        assert_eq!(
            h1_size.expect("h1"),
            docagent_layout::H1_SIZE_MAX_HU,
            "h1 size {h1_size:?} must be letter CSS 1.75rem"
        );
        let first = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, color, .. } if text.contains("September") => {
                Some((*y, *size, *color))
            }
            _ => None,
        });
        let second = list.ops.iter().find_map(|op| match op {
            Op::Text { y, text, color, .. } if text.contains("operations") => Some((*y, *color)),
            _ => None,
        });
        let (y1, s1, c1) = first.expect("date");
        let (y2, c2) = second.expect("dest");
        assert_eq!(
            c1,
            docagent_layout::BODY_FG,
            "body run must be letter CSS #134e4a, not UA black"
        );
        assert_eq!(c2, docagent_layout::BODY_FG);
        assert_ne!(c1, [0, 0, 0, 255]);
        let step = y2.saturating_sub(y1);
        assert!(
            step >= s1.saturating_add(docagent_layout::BODY_AFTER_HU),
            "PDF body step {step} must include letter CSS 0.7rem after"
        );
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(claims_pdfa(&pdf));
    }

    #[test]
    fn letter_h1_letter_spacing_is_weasyprint_neg_02em() {
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
        doc.sections.push(section);
        let fonts = FontSet::bundled();
        let tree = layout_document(&doc, &fonts);
        let h1 = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.text.contains("Northwind Freight"))
            .expect("h1");
        let h2 = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.text.contains("Delivery confirmation"))
            .expect("h2");
        let h1_natural =
            docagent_font::shape(&fonts, "Northwind Freight", docagent_layout::H1_SIZE_MAX_HU)
                .width;
        let h2_natural = docagent_font::shape(
            &fonts,
            "Delivery confirmation",
            docagent_layout::H2_SIZE_MAX_HU,
        )
        .width;
        let gaps = i32::try_from("Northwind Freight".chars().count().saturating_sub(1))
            .unwrap_or(i32::MAX);
        assert_eq!(docagent_layout::H1_LETTER_SPACING_HU, -42);
        assert_eq!(
            docagent_layout::h1_letter_spacing(docagent_layout::H1_SIZE_MAX_HU),
            docagent_layout::H1_LETTER_SPACING_HU
        );
        assert_eq!(
            h1.width,
            h1_natural.saturating_add(docagent_layout::H1_LETTER_SPACING_HU.saturating_mul(gaps)),
            "h1 width {} must bake letter CSS -.02em, not UA normal {}",
            h1.width,
            h1_natural
        );
        assert!(
            h1.width < h1_natural,
            "h1 must track tighter than UA 0 letter-spacing"
        );
        assert_eq!(
            h2.width, h2_natural,
            "letter.css letter-spacing is h1-only, not h2"
        );
        let list = paint(&tree);
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Text { text, size, .. }
                        if text.contains("Northwind Freight")
                            && *size == docagent_layout::H1_SIZE_MAX_HU
                )
            }),
            "h1 must stay one Text run so paint tests keep the title: {:?}",
            list.ops
        );
        let pdf = to_pdfa(&list, &fonts).expect("pdf");
        assert!(claims_pdfa(&pdf));
        assert!(pdf.starts_with(b"%PDF"));
    }

    #[test]
    fn letter_heading_line_height_pdfa_is_weasyprint_12() {
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
        doc.sections.push(section);
        let fonts = FontSet::bundled();
        let tree = layout_document(&doc, &fonts);
        let h1 = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.text.contains("Northwind Freight"))
            .expect("h1");
        let h2 = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.text.contains("Delivery confirmation"))
            .expect("h2");
        let h1_natural = docagent_font::line_height(&fonts, h1.font_size, 100);
        let h2_natural = docagent_font::line_height(&fonts, h2.font_size, 100);
        assert_eq!(docagent_layout::HEADING_LEADING_PCT, 120);
        assert_eq!(h1.height, docagent_layout::heading_line_height(h1.font_size));
        assert_eq!(h2.height, docagent_layout::heading_line_height(h2.font_size));
        assert_eq!(h1.height, 2520);
        assert_eq!(h2.height, 1656);
        assert!(h1.height < h1_natural, "PDF h1 1.2em must beat Noto natural");
        assert!(h2.height < h2_natural, "PDF h2 1.2em must beat Noto natural");
        let list = paint(&tree);
        let h1_paint = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, .. } if text.contains("Northwind Freight") => {
                Some((*y, *size))
            }
            _ => None,
        });
        let h2_paint = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, .. } if text.contains("Delivery confirmation") => {
                Some((*y, *size))
            }
            _ => None,
        });
        let (h1y, h1size) = h1_paint.expect("h1");
        let (h2y, h2size) = h2_paint.expect("h2");
        assert_eq!(h1size, docagent_layout::H1_SIZE_MAX_HU);
        assert_eq!(h2size, docagent_layout::H2_SIZE_MAX_HU);
        let stack = h2y.saturating_sub(h1y);
        assert!(
            stack < h1_natural.saturating_add(h2_natural),
            "PDF h1/h2 stack {stack} must be tighter than Noto (asc+desc)"
        );
        let pdf = to_pdfa(&list, &fonts).expect("pdf");
        assert!(claims_pdfa(&pdf));
        assert!(pdf.starts_with(b"%PDF"));
    }

    #[test]
    fn letter_h2_rule_pdfa_is_weasyprint_full_content_width() {
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
        doc.sections.push(section);
        let fonts = FontSet::bundled();
        let tree = layout_document(&doc, &fonts);
        let page_w = tree.pages[0].width;
        let content_w = page_w.saturating_sub(docagent_layout::PAGE_MARGIN_X_HU.saturating_mul(2));
        let h2 = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.text.contains("Delivery confirmation"))
            .expect("h2");
        let list = paint(&tree);
        let h1_rule = list.ops.iter().find_map(|op| match op {
            Op::FillRect { x, w, h, color, .. }
                if *color == docagent_layout::BODY_FG
                    && *h == docagent_layout::H1_RULE_HU
                    && *w > 1000 =>
            {
                Some((*x, *w))
            }
            _ => None,
        });
        let h2_rule = list.ops.iter().find_map(|op| match op {
            Op::FillRect { x, w, h, color, .. }
                if *color == docagent_layout::TH_RULE
                    && *h == docagent_layout::H2_RULE_HU
                    && *w > 80 =>
            {
                Some((*x, *w))
            }
            _ => None,
        });
        let (h1x, h1w) = h1_rule.expect("h1 3px rule");
        let (rx, rw) = h2_rule.expect("h2 2px rule");
        assert_eq!(
            rx, docagent_layout::PAGE_MARGIN_X_HU,
            "PDF h2 border-bottom x {rx} must be letter CSS content left, not text x {}",
            h2.x
        );
        assert_eq!(
            rw, content_w,
            "PDF h2 rule width {rw} must be letter CSS block border-bottom, not text span {}",
            h2.width
        );
        assert_eq!(rw, h1w, "h2 border-bottom must span the same content box as h1");
        assert_eq!(h1x, docagent_layout::PAGE_MARGIN_X_HU);
        assert!(
            rw > h2.width,
            "PDF h2 rule {rw} must outrun heading text {}",
            h2.width
        );
        assert_eq!(docagent_layout::H2_RULE_HU, 150);
        assert_eq!(docagent_layout::H2_RULE_GAP_HU, 240);
        assert_eq!(docagent_layout::TH_RULE, [13, 148, 136, 255]);
        assert_eq!(docagent_layout::PAGE_MARGIN_X_HU, 3300);
        assert_eq!(docagent_layout::H1_RULE_HU, 225);
        assert_eq!(docagent_layout::H1_AFTER_HU, 720);
        assert_eq!(docagent_layout::H2_BEFORE_HU, 1320);
        assert_eq!(docagent_layout::H2_AFTER_HU, 540);
        assert_eq!(docagent_layout::CELL_PAD_X_HU, 780);
        let pdf = to_pdfa(&list, &fonts).expect("pdf");
        assert!(claims_pdfa(&pdf));
        assert!(pdf.starts_with(b"%PDF"));
    }

    fn bullet_para(text: &str, level: u8) -> Paragraph {
        let mut p = Paragraph::from_text(text);
        p.space_after = 80;
        p.indent_left = 1440i32.saturating_mul(i32::from(level) + 1);
        p.numbering = Some(docagent_model::NumberingRef {
            definition_id: 0,
            level,
            start: None,
            format: Some(docagent_model::NumberFormat::Bullet),
        });
        p
    }

    #[test]
    fn letter_list_item_gap_is_weasyprint_letter_css() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Paragraph(bullet_para("Hash the input", 0)));
        section.body.push(Block::Paragraph(bullet_para("Run Convert", 0)));
        let mut after = Paragraph::from_text("DocAgent runtime");
        after.space_after = 200;
        section.body.push(Block::Paragraph(after));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let first = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, .. } if text.contains("Hash") => Some((*y, *size)),
            _ => None,
        });
        let second = list.ops.iter().find_map(|op| match op {
            Op::Text { y, text, .. } if text.contains("Convert") => Some(*y),
            _ => None,
        });
        let after = list.ops.iter().find_map(|op| match op {
            Op::Text { y, text, .. } if text.contains("runtime") => Some(*y),
            _ => None,
        });
        let (y1, s1) = first.expect("first");
        let y2 = second.expect("second");
        let y3 = after.expect("after");
        let item_step = y2.saturating_sub(y1);
        assert!(
            item_step >= s1.saturating_add(docagent_layout::LIST_ITEM_AFTER_HU),
            "PDF list step {item_step} must include letter CSS 0.35rem"
        );
        assert!(
            item_step
                <= s1
                    .saturating_mul(2)
                    .saturating_add(docagent_layout::LIST_ITEM_AFTER_HU),
            "PDF list step {item_step} too sparse vs letter CSS 0.35rem"
        );
        const {
            assert!(docagent_layout::LIST_ITEM_AFTER_HU < docagent_layout::BODY_AFTER_HU)
        };
        let after_step = y3.saturating_sub(y2);
        assert!(
            after_step >= s1.saturating_add(docagent_layout::LIST_AFTER_HU),
            "PDF after-list {after_step} must be letter CSS 1rem"
        );
        assert_eq!(docagent_layout::H1_SIZE_MAX_HU, 2100);
        assert_eq!(docagent_layout::CELL_PAD_X_HU, 780);
        assert_eq!(docagent_layout::IMAGE_AFTER_HU, 840);
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(claims_pdfa(&pdf));
    }

    #[test]
    fn letter_nested_list_gap_is_weasyprint_025rem() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(bullet_para("Receiving dock is clear", 0)));
        section
            .body
            .push(Block::Paragraph(bullet_para("Bay 4 seals intact", 1)));
        section
            .body
            .push(Block::Paragraph(bullet_para("Capsule hashes travel", 0)));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let dock = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, .. } if text.contains("dock") => Some((*y, *size)),
            _ => None,
        });
        let bay = list.ops.iter().find_map(|op| match op {
            Op::Text { y, text, .. } if text.contains("Bay") => Some(*y),
            _ => None,
        });
        let capsule = list.ops.iter().find_map(|op| match op {
            Op::Text { y, text, .. } if text.contains("Capsule") => Some(*y),
            _ => None,
        });
        let (y1, s1) = dock.expect("parent");
        let y2 = bay.expect("nested");
        let y3 = capsule.expect("sibling");
        let nested_step = y2.saturating_sub(y1);
        let sibling_step = y3.saturating_sub(y2);
        assert_eq!(
            sibling_step.saturating_sub(nested_step),
            docagent_layout::LIST_ITEM_AFTER_HU
                .saturating_sub(docagent_layout::LIST_NESTED_GAP_HU),
            "PDF nested step {nested_step} must be 0.10rem tighter than sibling {sibling_step}"
        );
        assert!(
            nested_step >= s1.saturating_add(docagent_layout::LIST_NESTED_GAP_HU),
            "PDF nested step {nested_step} must include letter CSS 0.25rem"
        );
        assert!(
            nested_step < sibling_step,
            "nested 0.25rem must read tighter than sibling 0.35rem"
        );
        assert_eq!(docagent_layout::LIST_NESTED_GAP_HU, 300);
        assert_eq!(docagent_layout::LIST_ITEM_AFTER_HU, 420);
        assert_eq!(docagent_layout::LIST_INDENT_HU, 1500);
        assert_eq!(docagent_layout::LIST_AFTER_HU, 1200);
        const {
            assert!(docagent_layout::LIST_NESTED_GAP_HU < docagent_layout::LIST_ITEM_AFTER_HU)
        };
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(claims_pdfa(&pdf));
    }

    #[test]
    fn letter_page_margin_is_weasyprint_25rem_275rem() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let date = list.ops.iter().find_map(|op| match op {
            Op::Text { x, y, text, .. } if text.contains("September") => Some((*x, *y)),
            _ => None,
        });
        let (x, y) = date.expect("body");
        assert_eq!(
            x,
            docagent_layout::PAGE_MARGIN_X_HU,
            "PDF body x {x} must be letter CSS @page 2.75rem, not Hangul 30mm"
        );
        assert!(
            (docagent_layout::PAGE_MARGIN_Y_HU..docagent_model::DEFAULT_MARGIN_HU).contains(&y),
            "PDF baseline y {y} must sit in letter CSS @page 2.5rem, not Hangul 30mm"
        );
        assert_eq!(docagent_layout::PAGE_MARGIN_X_HU, 3300);
        assert_eq!(docagent_layout::PAGE_MARGIN_Y_HU, 3000);
        assert_ne!(
            docagent_layout::PAGE_MARGIN_X_HU,
            docagent_model::DEFAULT_MARGIN_HU,
            "Hangul DEFAULT_MARGIN_HU 8504 is a sparse frame next to WeasyPrint"
        );
        assert_ne!(
            docagent_layout::PAGE_MARGIN_X_HU,
            75 * docagent_model::HU_PER_INCH / 96,
            "WeasyPrint UA @page margin 75px is not letter.html"
        );
        const {
            assert!(docagent_layout::PAGE_MARGIN_X_HU < docagent_model::DEFAULT_MARGIN_HU)
        };
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(claims_pdfa(&pdf));
        let sparse = {
            let mut doc = Document::new();
            let mut section = Section::default();
            // One HU off Hangul default so layout keeps the sparse IR frame.
            section.page.margin_left = docagent_model::DEFAULT_MARGIN_HU + 1;
            section.page.margin_right = docagent_model::DEFAULT_MARGIN_HU + 1;
            section.page.margin_top = docagent_model::DEFAULT_MARGIN_HU + 1;
            section.page.margin_bottom = docagent_model::DEFAULT_MARGIN_HU + 1;
            section
                .body
                .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
            doc.sections.push(section);
            doc
        };
        assert_ne!(
            pdf,
            to_pdfa(
                &paint(&layout_document(&sparse, &FontSet::bundled())),
                &FontSet::bundled()
            )
            .expect("sparse"),
            "letter @page 2.5rem/2.75rem must change PDF bytes vs Hangul 30mm"
        );
    }

    #[test]
    fn letter_quote_and_code_spacing_is_weasyprint_letter_css() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut before = Paragraph::from_text("Wet cargo is H2O only.");
        before.space_after = 200;
        section.body.push(Block::Paragraph(before));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        quote.space_before = 80;
        quote.space_after = 200;
        section.body.push(Block::Paragraph(quote));
        let mut code = Paragraph::from_text("docagent prove examples/letter.md");
        code.code_block = true;
        code.space_before = 80;
        code.space_after = 200;
        section.body.push(Block::Paragraph(code));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let before = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, .. } if text.contains("Wet cargo") => Some((*y, *size)),
            _ => None,
        });
        let quote = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, .. } if text.contains("Replay") => Some((*y, *size)),
            _ => None,
        });
        let code = list.ops.iter().find_map(|op| match op {
            Op::Text { x, y, text, .. } if text.contains("docagent prove") => Some((*x, *y)),
            _ => None,
        });
        let (y0, s0) = before.expect("before");
        let (qy, qs) = quote.expect("quote");
        let (cx, cy) = code.expect("code");
        let into_quote = qy.saturating_sub(y0);
        assert!(
            into_quote
                >= s0
                    .saturating_add(docagent_layout::BODY_AFTER_HU.max(docagent_layout::QUOTE_BEFORE_HU))
                    .saturating_add(docagent_layout::QUOTE_PAD_Y_HU),
            "PDF quote step {into_quote} jammed vs letter CSS blockquote"
        );
        let into_code = cy.saturating_sub(qy);
        assert!(
            into_code
                >= qs
                    .saturating_add(docagent_layout::QUOTE_PAD_Y_HU)
                    .saturating_add(
                        docagent_layout::QUOTE_AFTER_HU.max(docagent_layout::CODE_BEFORE_HU)
                    )
                    .saturating_add(docagent_layout::CODE_PAD_Y_HU),
            "PDF quote-to-code {into_code} jammed vs letter CSS"
        );
        assert!(
            cx > docagent_layout::CODE_PAD_X_HU,
            "PDF code x {cx} must include 1rem pad"
        );
        let qcolor = list.ops.iter().find_map(|op| match op {
            Op::Text { color, text, .. } if text.contains("Replay") => Some(*color),
            _ => None,
        });
        assert_eq!(
            qcolor.expect("quote color"),
            docagent_layout::QUOTE_FG,
            "blockquote text must be letter CSS #134e4a"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, w, .. }
                        if *color == docagent_layout::QUOTE_FG
                            && *w == docagent_layout::QUOTE_BAR_W_HU
                )
            }),
            "PDF quote bar must be letter CSS 4px"
        );
        assert_eq!(docagent_layout::QUOTE_FG, [19, 78, 74, 255]);
        assert_eq!(docagent_layout::QUOTE_BAR_W_HU, 300);
        assert_eq!(docagent_layout::CELL_PAD_Y_HU, 480);
        assert_eq!(docagent_layout::H2_SIZE_MAX_HU, 1380);
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(claims_pdfa(&pdf));
        assert_ne!(
            pdf,
            to_pdfa(
                &paint(&layout_document(
                    &{
                        let mut plain = Document::new();
                        let mut section = Section::default();
                        section.body.push(Block::Paragraph(Paragraph::from_text(
                            "Wet cargo is H2O only.",
                        )));
                        section.body.push(Block::Paragraph(Paragraph::from_text(
                            "Replay is evidence. Same fonts, same bytes.",
                        )));
                        section.body.push(Block::Paragraph(Paragraph::from_text(
                            "docagent prove examples/letter.md",
                        )));
                        plain.sections.push(section);
                        plain
                    },
                    &FontSet::bundled()
                )),
                &FontSet::bundled()
            )
            .expect("plain"),
            "quote/code spacing must change PDF bytes"
        );
    }

    #[test]
    fn letter_code_block_pad_x_is_weasyprint_1rem_both_sides() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut code = Paragraph::from_text("x ".repeat(400));
        code.code_block = true;
        code.space_before = 80;
        code.space_after = 200;
        section.body.push(Block::Paragraph(code));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let list = paint(&tree);
        let origin = docagent_layout::PAGE_MARGIN_X_HU;
        let content_w = tree.pages[0]
            .width
            .saturating_sub(docagent_layout::PAGE_MARGIN_X_HU.saturating_mul(2));
        let right_limit = origin
            .saturating_add(content_w)
            .saturating_sub(docagent_layout::CODE_PAD_X_HU);
        let lines: Vec<_> = tree.pages[0]
            .lines
            .iter()
            .filter(|l| l.width > 0 && !l.text.is_empty())
            .collect();
        assert!(
            lines.len() >= 2,
            "PDF pre must wrap so right pad is observable"
        );
        for line in &lines {
            assert_eq!(
                line.x,
                origin.saturating_add(docagent_layout::CODE_PAD_X_HU),
                "PDF code x {} must stay letter CSS 1rem left pad",
                line.x.saturating_sub(origin)
            );
            assert!(
                line.x.saturating_add(line.width) <= right_limit,
                "PDF code right {} must keep letter CSS 1rem right pad",
                line.x.saturating_add(line.width)
            );
        }
        let texts: Vec<i32> = list
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::Text { x, text, .. } if !text.is_empty() => Some(*x),
                _ => None,
            })
            .collect();
        assert!(texts.len() >= 2, "PDF paint must emit wrapped pre runs");
        assert!(
            texts
                .iter()
                .all(|x| *x == origin.saturating_add(docagent_layout::CODE_PAD_X_HU)),
            "PDF pre glyphs must sit on the 1rem left pad: {texts:?}"
        );
        let fill = list.ops.iter().find_map(|op| match op {
            Op::FillRect { x, w, color, .. }
                if *color == [204, 251, 241, 255] && *w > 10000 =>
            {
                Some((*x, *w))
            }
            _ => None,
        });
        let (fx, fw) = fill.expect("code fill");
        assert_eq!(fx, origin);
        assert_eq!(
            fw, content_w,
            "PDF pre fill {fw} must span the content box, not shrink by 1rem"
        );
        assert_eq!(docagent_layout::CODE_PAD_X_HU, 1200);
        assert_eq!(docagent_layout::CODE_PAD_Y_HU, 900);
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(claims_pdfa(&pdf));
        assert_ne!(
            pdf,
            to_pdfa(
                &paint(&layout_document(
                    &{
                        let mut plain = Document::new();
                        let mut section = Section::default();
                        section.body.push(Block::Paragraph(Paragraph::from_text(
                            "docagent prove examples/letter.md",
                        )));
                        plain.sections.push(section);
                        plain
                    },
                    &FontSet::bundled()
                )),
                &FontSet::bundled()
            )
            .expect("short"),
            "1rem right pad wrap must change PDF bytes vs a short pre"
        );
    }

    #[test]
    fn letter_pdfa_body_quote_th_tasks_and_24pt_still_emit() {
        let (w, h) = mark_png_hu();
        let (ew, eh) = docagent_layout::min_on_page_size(w, h);
        assert_eq!(
            docagent_layout::MIN_IMAGE_EDGE_HU,
            24 * docagent_model::HU_PER_POINT
        );
        assert_eq!(docagent_layout::MIN_IMAGE_EDGE_HU, 2400);
        assert!(ew >= docagent_layout::MIN_IMAGE_EDGE_HU);
        assert!(eh >= docagent_layout::MIN_IMAGE_EDGE_HU);
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![mark_run(w, h)];
        section.body.push(Block::Paragraph(img_p));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        let mut table = docagent_model::Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        section
            .body
            .push(Block::Paragraph(task_para("Receiving dock is clear", true)));
        doc.sections.push(section);
        let fonts = FontSet::bundled();
        let list = paint(&layout_document(&doc, &fonts));
        let image = list.ops.iter().find_map(|op| match op {
            Op::Image { w, h, bytes, mime, y, .. }
                if bytes.as_slice() == MARK_PNG && mime == "image/png" =>
            {
                Some((*w, *h, *y))
            }
            _ => None,
        });
        let (iw, ih, iy) = image.expect("24pt mark");
        assert_eq!((iw, ih), (ew, eh), "PDF/A must keep the 24pt on-page floor");
        let date = list.ops.iter().find_map(|op| match op {
            Op::Text { y, color, text, .. } if text.contains("6 September") => {
                Some((*y, *color))
            }
            _ => None,
        });
        let (by, bcolor) = date.expect("body");
        assert_eq!(
            bcolor,
            docagent_layout::BODY_FG,
            "body run must be letter CSS #134e4a, not UA black"
        );
        assert_ne!(bcolor, [0, 0, 0, 255]);
        assert_eq!(docagent_layout::BODY_FG, [19, 78, 74, 255]);
        assert_ne!(
            docagent_layout::BODY_FG,
            [0, 0, 0, 255],
            "body must not regress to UA black"
        );
        assert!(
            by >= iy.saturating_add(ih),
            "body overlaps 24pt mark"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, w, h, .. }
                        if *color == docagent_layout::TH_BG && *w > 80 && *h > 80
                )
            }),
            "th fill must not regress"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Text { text, color, bold: true, .. }
                        if text.contains("Hop") && *color == docagent_layout::TH_FG
                )
            }),
            "th text must be letter CSS #134e4a bold"
        );
        assert_eq!(docagent_layout::TH_BG, [204, 251, 241, 255]);
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Text { text, color, .. }
                        if text.contains("Replay is evidence")
                            && *color == docagent_layout::QUOTE_FG
                )
            }),
            "blockquote text must be letter CSS #134e4a"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, w, h, .. }
                        if *color == docagent_layout::QUOTE_FG
                            && *w == docagent_layout::QUOTE_BAR_W_HU
                            && *h > 200
                )
            }),
            "quote bar must not regress"
        );
        assert_eq!(docagent_layout::QUOTE_BAR_W_HU, 300);
        assert_eq!(docagent_layout::QUOTE_FG, [19, 78, 74, 255]);
        let box_hu = docagent_layout::TASK_BOX_HU;
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, w, h, .. }
                        if *color == [19, 78, 74, 255] && *w == box_hu && *h == box_hu
                )
            }),
            "task box must not regress"
        );
        assert_eq!(docagent_layout::TASK_BOX_HU, 900);
        let pdf = to_pdfa(&list, &fonts).expect("pdf");
        assert!(pdf.starts_with(b"%PDF"));
        assert!(claims_pdfa(&pdf));
        let s = String::from_utf8_lossy(&pdf);
        assert!(has_image_xobject(&s), "24pt mark XObject missing");
        let mut plain = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        plain.sections.push(section);
        let plain_pdf = to_pdfa(
            &paint(&layout_document(&plain, &fonts)),
            &fonts,
        )
        .expect("plain");
        assert_ne!(
            pdf, plain_pdf,
            "body color + quote bar + th + tasks + 24pt must change PDF bytes"
        );
    }

    fn has_image_xobject(s: &str) -> bool {
        (s.contains("/Subtype /Image") || s.contains("/Subtype/Image"))
            && (s.contains("/Type /XObject") || s.contains("/Type/XObject"))
    }

    /// 1×1 RGB JPEG (SOI…EOI). Valid input for `Image::from_jpeg`.
    const PIXEL_JPEG: &[u8] = &[
        0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00, 0x00,
        0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xDB,
        0x00, 0x43, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x01,
        0x00, 0x01, 0x03, 0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01, 0xFF, 0xC4, 0x00,
        0x15, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x02, 0xFF, 0xC4, 0x00, 0x14, 0x10, 0x01, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xC4, 0x00,
        0x15, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x01, 0x02, 0xFF, 0xC4, 0x00, 0x14, 0x11, 0x01, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xDA, 0x00,
        0x0C, 0x03, 0x01, 0x00, 0x02, 0x11, 0x03, 0x11, 0x00, 0x3F, 0x00, 0x90, 0x02, 0xDF, 0xFF,
        0xD9,
    ];

    fn doc_with_jpeg() -> Document {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![docagent_model::Run {
            style: docagent_model::CharStyle::default(),
            content: docagent_model::RunContent::Inline(docagent_model::InlineObject::Image(
                docagent_model::ImageData {
                    bytes: PIXEL_JPEG.to_vec(),
                    mime: "image/jpeg".into(),
                    width: 1600,
                    height: 1600,
                    alt_text: Some("pixel".into()),
                    wrap: docagent_model::WrapMode::Inline,
                },
            )),
        }];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        doc
    }

    #[test]
    fn jpeg_image_embeds_in_pdfa() {
        let list = paint(&layout_document(&doc_with_jpeg(), &FontSet::bundled()));
        let (ew, eh) = docagent_layout::min_on_page_size(1600, 1600);
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Image { w, h, bytes, mime, .. }
                        if *w == ew && *h == eh && bytes.as_slice() == PIXEL_JPEG && mime == "image/jpeg"
                )
            }),
            "paint must forward JPEG bytes"
        );
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(pdf.starts_with(b"%PDF"));
        assert!(claims_pdfa(&pdf), "pdfa identifier missing");
        let s = String::from_utf8_lossy(&pdf);
        assert!(
            s.contains("/Image"),
            "PDF image XObject missing: {}",
            s.chars().take(400).collect::<String>()
        );
        assert!(s.contains("/Width"), "image width missing");
        assert!(s.contains("/Height"), "image height missing");
        let mut blank = Document::new();
        blank.sections.push(Section::default());
        let blank_pdf = to_pdfa(
            &paint(&layout_document(&blank, &FontSet::bundled())),
            &FontSet::bundled(),
        )
        .expect("pdf");
        assert!(
            pdf.len() > blank_pdf.len(),
            "embedded JPEG must enlarge PDF ({} vs {})",
            pdf.len(),
            blank_pdf.len()
        );
    }

    fn doc_with_float_mark() -> Document {
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Float(docagent_model::Float::Image(
            docagent_model::ImageData {
                bytes: MARK_PNG.to_vec(),
                mime: "image/png".into(),
                width: 1600,
                height: 1600,
                alt_text: Some("mark".into()),
                wrap: docagent_model::WrapMode::Square,
            },
        )));
        doc.sections.push(section);
        doc
    }

    #[test]
    fn float_image_embeds_in_pdfa() {
        let list = paint(&layout_document(
            &doc_with_float_mark(),
            &FontSet::bundled(),
        ));
        let (ew, eh) = docagent_layout::min_on_page_size(1600, 1600);
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Image { w, h, bytes, mime, .. }
                        if *w == ew && *h == eh && bytes.as_slice() == MARK_PNG && mime == "image/png"
                )
            }),
            "paint must forward float PNG bytes"
        );
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(pdf.starts_with(b"%PDF"));
        assert!(claims_pdfa(&pdf), "pdfa identifier missing");
        let s = String::from_utf8_lossy(&pdf);
        assert!(
            s.contains("/Image"),
            "PDF image XObject missing: {}",
            s.chars().take(400).collect::<String>()
        );
    }

    fn doc_with_table_cell_mark() -> Document {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = docagent_model::Table::from_cells(vec![vec![String::new()]]);
        let mut p = Paragraph::from_text("");
        p.runs = vec![docagent_model::Run {
            style: docagent_model::CharStyle::default(),
            content: docagent_model::RunContent::Inline(docagent_model::InlineObject::Image(
                docagent_model::ImageData {
                    bytes: MARK_PNG.to_vec(),
                    mime: "image/png".into(),
                    width: 1600,
                    height: 1600,
                    alt_text: Some("mark".into()),
                    wrap: docagent_model::WrapMode::Inline,
                },
            )),
        }];
        table.rows[0].cells[0].blocks = vec![Block::Paragraph(p)];
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        doc
    }

    #[test]
    fn table_cell_image_embeds_in_pdfa() {
        let list = paint(&layout_document(
            &doc_with_table_cell_mark(),
            &FontSet::bundled(),
        ));
        let (ew, eh) = docagent_layout::min_on_page_size(1600, 1600);
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Image { w, h, bytes, mime, .. }
                        if *w == ew && *h == eh && bytes.as_slice() == MARK_PNG && mime == "image/png"
                )
            }),
            "paint must forward table-cell PNG bytes"
        );
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(pdf.starts_with(b"%PDF"));
        assert!(claims_pdfa(&pdf), "pdfa identifier missing");
        let s = String::from_utf8_lossy(&pdf);
        assert!(
            s.contains("/Image"),
            "PDF image XObject missing: {}",
            s.chars().take(400).collect::<String>()
        );
    }

    #[test]
    fn section_header_embeds_in_pdfa() {
        let mut doc = Document::new();
        let mut section = Section {
            header: Some(docagent_model::HeaderFooter {
                blocks: vec![Block::Paragraph(Paragraph::from_text("HEAD"))],
                different_first: None,
                different_odd_even: None,
            }),
            ..Section::default()
        };
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("body")));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        assert!(
            list.ops
                .iter()
                .any(|op| matches!(op, Op::Text { text, .. } if text.contains("HEAD"))),
            "paint must emit header text"
        );
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(pdf.starts_with(b"%PDF"));
        assert!(claims_pdfa(&pdf), "pdfa identifier missing");
    }

    #[test]
    fn section_footer_embeds_in_pdfa() {
        let mut doc = Document::new();
        let mut section = Section {
            footer: Some(docagent_model::HeaderFooter {
                blocks: vec![Block::Paragraph(Paragraph::from_text("FOOT"))],
                different_first: None,
                different_odd_even: None,
            }),
            ..Section::default()
        };
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("body")));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        assert!(
            list.ops
                .iter()
                .any(|op| matches!(op, Op::Text { text, .. } if text.contains("FOOT"))),
            "paint must emit footer text"
        );
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(pdf.starts_with(b"%PDF"));
        assert!(claims_pdfa(&pdf), "pdfa identifier missing");
    }

    fn doc_with_header_mark() -> Document {
        let mut doc = Document::new();
        let mut p = Paragraph::from_text("");
        p.runs = vec![docagent_model::Run {
            style: docagent_model::CharStyle::default(),
            content: docagent_model::RunContent::Inline(docagent_model::InlineObject::Image(
                docagent_model::ImageData {
                    bytes: MARK_PNG.to_vec(),
                    mime: "image/png".into(),
                    width: 1600,
                    height: 1600,
                    alt_text: Some("mark".into()),
                    wrap: docagent_model::WrapMode::Inline,
                },
            )),
        }];
        let mut section = Section {
            header: Some(docagent_model::HeaderFooter {
                blocks: vec![Block::Paragraph(p)],
                different_first: None,
                different_odd_even: None,
            }),
            ..Section::default()
        };
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("body")));
        doc.sections.push(section);
        doc
    }

    #[test]
    fn section_header_image_embeds_in_pdfa() {
        let list = paint(&layout_document(
            &doc_with_header_mark(),
            &FontSet::bundled(),
        ));
        let (ew, eh) = docagent_layout::min_on_page_size(1600, 1600);
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Image { w, h, bytes, mime, .. }
                        if *w == ew && *h == eh && bytes.as_slice() == MARK_PNG && mime == "image/png"
                )
            }),
            "paint must forward header PNG bytes"
        );
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(pdf.starts_with(b"%PDF"));
        assert!(claims_pdfa(&pdf), "pdfa identifier missing");
        let s = String::from_utf8_lossy(&pdf);
        assert!(
            s.contains("/Image"),
            "PDF image XObject missing: {}",
            s.chars().take(400).collect::<String>()
        );
    }
}
