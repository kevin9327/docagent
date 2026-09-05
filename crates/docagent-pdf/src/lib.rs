//! PDF/A export from the display list. Floating-point is allowed here.

#![forbid(unsafe_code)]

use docagent_font::FontSet;
use docagent_paint::{DisplayList, Op};
use krilla::configure::{Archival, ConfigurationBuilder, Validator};
use krilla::geom::Point;
use krilla::metadata::{DateTime, Metadata};
use krilla::page::PageSettings;
use krilla::text::{Font, TextDirection};
use krilla::{Document, SerializeSettings};

const HU_PER_PT: f32 = 100.0;

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
            {
                let mut surface = page.surface();
                for op in &ops {
                    if let Op::Text {
                        x,
                        y,
                        size,
                        text,
                        color: _,
                    } = op
                    {
                        let mapped = docagent_font::retain_mapped_chars(fonts, text);
                        if mapped.is_empty() {
                            continue;
                        }
                        let pt = (*size as f32 / HU_PER_PT).max(1.0);
                        surface.draw_text(
                            Point::from_xy(*x as f32 / HU_PER_PT, *y as f32 / HU_PER_PT),
                            font.clone(),
                            pt,
                            &mapped,
                            false,
                            TextDirection::Auto,
                        );
                    }
                }
                surface.finish();
            }
            page.finish();
        }
    }
    document.finish().map_err(|e| format!("{e:?}"))
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
}
