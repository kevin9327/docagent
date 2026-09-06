//! PDF/A export from the display list. Floating-point is allowed here.

#![forbid(unsafe_code)]

use docagent_font::FontSet;
use docagent_paint::{DisplayList, Op};
use krilla::action::LinkAction;
use krilla::annotation::{Annotation, LinkAnnotation, Target};
use krilla::color::rgb;
use krilla::configure::{Archival, ConfigurationBuilder, Validator};
use krilla::geom::{PathBuilder, Point, Rect, Transform};
use krilla::metadata::{DateTime, Metadata};
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::Fill;
use krilla::text::{Font, TextDirection};
use krilla::{Document, SerializeSettings};

const HU_PER_PT: f32 = 100.0;
/// tan(~11.3°). Y-down shear so glyph tops lean right around the baseline.
const ITALIC_SHEAR: f32 = 0.20;

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
                        Op::Text {
                            x,
                            y,
                            size,
                            text,
                            color: _,
                            bold,
                            italic,
                        } => {
                            let mapped = docagent_font::retain_mapped_chars(fonts, text);
                            if mapped.is_empty() {
                                continue;
                            }
                            surface.set_fill(Some(Fill {
                                paint: rgb::Color::black().into(),
                                opacity: NormalizedF32::ONE,
                                rule: Default::default(),
                            }));
                            surface.set_stroke(None);
                            let pt = (*size as f32 / HU_PER_PT).max(1.0);
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
                            surface.draw_text(
                                Point::from_xy(px, py),
                                font.clone(),
                                pt,
                                &mapped,
                                false,
                                TextDirection::Auto,
                            );
                            if *bold {
                                // Synthetic bold: second fill offset ~3% of em.
                                let dx = (pt * 0.03).max(0.2);
                                surface.draw_text(
                                    Point::from_xy(px + dx, py),
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
                        Op::Link { x, y, w, h, uri } => {
                            if *w > 0 && *h > 0 && !uri.is_empty() {
                                links.push((*x, *y, *w, *h, uri.clone()));
                            }
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
                |op| matches!(op, Op::FillRect { color, w, h, .. } if *color == [0, 0, 0, 255] && *w > 0 && *h > 0)
            ),
            "table grid must paint black strokes"
        );
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(pdf.starts_with(b"%PDF"));
        assert!(claims_pdfa(&pdf));
        assert!(pdf.len() > 1000);
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
                        if *color == [19, 78, 74, 255] && *h == 200 && *w > 1000
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
        assert!(rs < base, "super size {rs} vs {base}");
        let base_y = list.ops.iter().find_map(|op| match op {
            Op::Text { text, y, .. } if text.contains("PDF") => Some(*y),
            _ => None,
        });
        assert!(
            ry < base_y.unwrap_or(ry),
            "super y {ry} vs {:?}",
            base_y
        );
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
        assert!(rs < base, "sub size {rs} vs {base}");
        let base_y = list.ops.iter().find_map(|op| match op {
            Op::Text { text, y, .. } if text == "H" => Some(*y),
            _ => None,
        });
        assert!(
            ry > base_y.unwrap_or(ry),
            "sub y {ry} vs {:?}",
            base_y
        );
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
                        if *color == [19, 78, 74, 255] && *h == 80 && *w > 80
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
                        if *color == [19, 78, 74, 255] && *h == 80 && *w > 80
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

    #[test]
    fn task_box_paints_non_grid_fill() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Replay the convert");
        p.numbering = Some(docagent_model::NumberingRef {
            definition_id: 2,
            level: 0,
            start: Some(1),
            format: Some(docagent_model::NumberFormat::Task),
        });
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let edges = list.ops.iter().filter(|op| {
            matches!(
                op,
                Op::FillRect { color, w, h, .. }
                    if *color == [19, 78, 74, 255] && *w != *h && *w > 0 && *h > 0
            )
        }).count();
        assert!(edges >= 4, "task box must paint edges");
        let pdf = to_pdfa(&list, &FontSet::bundled()).expect("pdf");
        assert!(claims_pdfa(&pdf));
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
                        if *color == [19, 78, 74, 255] && *h == 80 && *w > 10000
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
                    Op::FillRect { color, h, w, .. }
                        if *color == [19, 78, 74, 255] && *w == 80 && *h > 200
                )
            }),
            "quote bar must paint a tall fill"
        );
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
                        if *color == [19, 78, 74, 255] && *h == 80 && *w > 80
                )
            }),
            "strikethrough must paint a fill"
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
    }
}
