//! Single paint backend IR: a flat display list, rkyv-serializable.

#![forbid(unsafe_code)]

use docagent_layout::FragmentTree;
use rkyv::{Archive, Deserialize, Serialize};

#[derive(Archive, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum Op {
    Page {
        width: i32,
        height: i32,
    },
    FillRect {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        color: [u8; 4],
    },
    Text {
        x: i32,
        y: i32,
        size: i32,
        text: String,
        color: [u8; 4],
        bold: bool,
        italic: bool,
    },
    /// Clickable URI. Coordinates are the same HWPUNIT space as `FillRect`.
    Link {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        uri: String,
    },
    /// Inline PNG/JPEG. `w`/`h` are HWPUNIT; `bytes` are the original file.
    Image {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        bytes: Vec<u8>,
        mime: String,
    },
}

#[derive(Archive, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct DisplayList {
    pub ops: Vec<Op>,
}

impl DisplayList {
    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        rkyv::to_bytes::<rkyv::rancor::Error>(self)
            .map(|b| b.to_vec())
            .map_err(|e| e.to_string())
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<DisplayList, String> {
        rkyv::from_bytes::<DisplayList, rkyv::rancor::Error>(bytes).map_err(|e| e.to_string())
    }
}

pub fn paint(tree: &FragmentTree) -> DisplayList {
    let mut ops = Vec::new();
    for page in &tree.pages {
        ops.push(Op::Page {
            width: page.width,
            height: page.height,
        });
        ops.push(Op::FillRect {
            x: 0,
            y: 0,
            w: page.width,
            h: page.height,
            color: [255, 255, 255, 255],
        });
        for rect in page.rects.iter().chain(page.strokes.iter()) {
            if rect.fill[3] == 0 {
                continue;
            }
            ops.push(Op::FillRect {
                x: rect.x,
                y: rect.y,
                w: rect.width,
                h: rect.height,
                color: rect.fill,
            });
        }
        for line in &page.lines {
            if let Some(img) = &line.image {
                if img.width > 0 && img.height > 0 && !img.bytes.is_empty() {
                    ops.push(Op::Image {
                        x: line.x,
                        y: line.y,
                        w: img.width,
                        h: img.height,
                        bytes: img.bytes.clone(),
                        mime: img.mime.clone(),
                    });
                }
                continue;
            }
            ops.push(Op::Text {
                x: line.x,
                y: line.y + line.baseline,
                size: line.font_size,
                text: line.text.clone(),
                color: if line.href.is_some() {
                    [13, 148, 136, 255]
                } else {
                    [0, 0, 0, 255]
                },
                bold: line.bold,
                italic: line.italic,
            });
            if let Some((x, y, w, h, uri)) = line.link_hit() {
                ops.push(Op::Link {
                    x,
                    y,
                    w,
                    h,
                    uri: uri.to_string(),
                });
            }
        }
    }
    DisplayList { ops }
}

#[cfg(test)]
mod tests {
    use super::*;
    use docagent_font::FontSet;
    use docagent_layout::layout_document;
    use docagent_model::{Block, Document, Paragraph, Section};

    #[test]
    fn paint_roundtrips_through_rkyv() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("paint")));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let list = paint(&tree);
        let bytes = list.to_bytes().unwrap();
        let back = DisplayList::from_bytes(&bytes).unwrap();
        assert_eq!(list, back);
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

    #[test]
    fn image_op_roundtrips_through_rkyv() {
        let list = DisplayList {
            ops: vec![Op::Image {
                x: 100,
                y: 200,
                w: 1600,
                h: 1600,
                bytes: MARK_PNG.to_vec(),
                mime: "image/png".into(),
            }],
        };
        let back = DisplayList::from_bytes(&list.to_bytes().unwrap()).unwrap();
        assert_eq!(list, back);
    }

    #[test]
    fn paint_forwards_bold_on_text_ops() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("plain ");
        let mut bold = docagent_model::Run::text("GPU-free");
        bold.style.bold = true;
        p.runs.push(bold);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let list = paint(&tree);
        assert!(list.ops.iter().any(|op| {
            matches!(op, Op::Text { text, bold: true, .. } if text.contains("GPU-free"))
        }));
        assert!(list.ops.iter().any(|op| {
            matches!(op, Op::Text { text, bold: false, .. } if text.contains("plain"))
        }));
    }

    #[test]
    fn paint_forwards_italic_on_text_ops() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("plain ");
        let mut italic = docagent_model::Run::text("byte-for-byte");
        italic.style.italic = true;
        p.runs.push(italic);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let list = paint(&tree);
        assert!(list.ops.iter().any(|op| {
            matches!(op, Op::Text { text, italic: true, .. } if text.contains("byte-for-byte"))
        }));
        assert!(list.ops.iter().any(|op| {
            matches!(op, Op::Text { text, italic: false, .. } if text.contains("plain"))
        }));
    }

    #[test]
    fn paint_forwards_link_ops() {
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
                    Op::Link { uri, w, h, .. }
                        if uri == "https://github.com/kevin9327/docagent" && *w > 0 && *h > 0
                )
            }),
            "link op missing: {:?}",
            list.ops
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Text { text, color, .. }
                        if text.contains("DocAgent") && *color == [13, 148, 136, 255]
                )
            }),
            "link text must be teal: {:?}",
            list.ops
        );
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

    fn boxes_overlap(a: (i32, i32, i32, i32), b: (i32, i32, i32, i32)) -> bool {
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

    #[test]
    fn paint_emits_image_ops() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("mark");
        p.runs.push(mark_run(1600, 1600));
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let (ew, eh) = docagent_layout::min_on_page_size(1600, 1600);
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Image { w, h, bytes, mime, .. }
                        if *w == ew && *h == eh && bytes == MARK_PNG && mime == "image/png"
                )
            }),
            "image op missing"
        );
    }

    #[test]
    fn paint_letter_mark_png_uses_minimum_on_page_size_without_overlapping_body() {
        assert_eq!(png_ihdr_px(MARK_PNG), (16, 16), "fixture is 16×16 px");
        let (w, h) = mark_png_hu();
        assert_eq!((w, h), (px_to_hu_96(16), px_to_hu_96(16)));
        assert!(w < docagent_layout::MIN_IMAGE_EDGE_HU);
        let (ew, eh) = docagent_layout::min_on_page_size(w, h);
        assert!(ew >= docagent_layout::MIN_IMAGE_EDGE_HU);
        assert!(eh >= docagent_layout::MIN_IMAGE_EDGE_HU);
        assert!(ew >= docagent_model::HU_PER_INCH / 4);
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut heading = Paragraph::from_text("Northwind Freight");
        heading.outline_level = Some(1);
        heading.runs[0].style.size = 1800;
        heading.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(heading));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![mark_run(w, h)];
        section.body.push(Block::Paragraph(img_p));
        section.body.push(Block::Paragraph(Paragraph::from_text(
            "The shipment left Busan on 4 September",
        )));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
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
            "paint must use the 24pt on-page floor, not 16px at 96dpi"
        );
        let body = list.ops.iter().find_map(|op| match op {
            Op::Text { x, y, size, text, .. } if text.contains("shipment") => {
                Some((*x, *y, *size))
            }
            _ => None,
        });
        let (bx, by, bsize) = body.expect("body text op missing");
        let img_box = (ix, iy, iw, ih);
        let body_box = (bx, by.saturating_sub(bsize), 1, bsize.max(1));
        assert!(
            by.saturating_sub(bsize) >= iy.saturating_add(ih),
            "body baseline {by} size {bsize} overlaps image y {iy} height {ih}"
        );
        assert!(
            !boxes_overlap(img_box, body_box),
            "image {img_box:?} overlaps body {body_box:?}"
        );
    }

    #[test]
    fn paint_letter_page_uses_dense_type_and_table() {
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
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let h1_size = list.ops.iter().find_map(|op| match op {
            Op::Text { size, text, .. } if text.contains("Northwind") => Some(*size),
            _ => None,
        });
        let h2_size = list.ops.iter().find_map(|op| match op {
            Op::Text { size, text, .. } if text.contains("Delivery") => Some(*size),
            _ => None,
        });
        assert!(
            h1_size.expect("h1") <= docagent_layout::H1_SIZE_MAX_HU,
            "h1 paint size {h1_size:?}"
        );
        assert!(
            h2_size.expect("h2") <= docagent_layout::H2_SIZE_MAX_HU,
            "h2 paint size {h2_size:?}"
        );
        let image = list.ops.iter().find_map(|op| match op {
            Op::Image { x, y, w, h, bytes, mime }
                if bytes.as_slice() == MARK_PNG && mime == "image/png" =>
            {
                Some((*x, *y, *w, *h))
            }
            _ => None,
        });
        let (ix, iy, iw, ih) = image.expect("mark");
        assert_eq!((iw, ih), (ew, eh));
        let body = list.ops.iter().find_map(|op| match op {
            Op::Text { x, y, size, text, .. } if text.contains("shipment") => {
                Some((*x, *y, *size))
            }
            _ => None,
        });
        let (bx, by, bsize) = body.expect("body");
        let img_box = (ix, iy, iw, ih);
        let body_box = (bx, by.saturating_sub(bsize), 1, bsize.max(1));
        assert!(
            by.saturating_sub(bsize) >= iy.saturating_add(ih),
            "body overlaps mark"
        );
        assert!(!boxes_overlap(img_box, body_box));
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, w, h, .. }
                        if *color == [204, 251, 241, 255] && *w > 80 && *h > 80
                )
            }),
            "table header fill missing"
        );
        let h1_y = list.ops.iter().find_map(|op| match op {
            Op::Text { y, text, .. } if text.contains("Northwind") => Some(*y),
            _ => None,
        });
        let h2_y = list.ops.iter().find_map(|op| match op {
            Op::Text { y, text, .. } if text.contains("Delivery") => Some(*y),
            _ => None,
        });
        let gap = h2_y.expect("h2 y").saturating_sub(h1_y.expect("h1 y"));
        assert!(
            gap <= 400
                + docagent_layout::H1_SIZE_MAX_HU
                + docagent_layout::H2_SIZE_MAX_HU,
            "heading stack too airy: {gap}"
        );
    }

    #[test]
    fn paint_emits_table_cell_image_ops() {
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
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let (ew, eh) = docagent_layout::min_on_page_size(1600, 1600);
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Image { w, h, bytes, mime, .. }
                        if *w == ew && *h == eh && bytes == MARK_PNG && mime == "image/png"
                )
            }),
            "table-cell image op missing"
        );
    }
}
