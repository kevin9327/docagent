//! Single paint backend IR: a flat display list, rkyv-serializable.

#![forbid(unsafe_code)]

use docagent_layout::{is_text_decoration, FragmentTree};
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
        let mut decorations = Vec::new();
        for rect in page.rects.iter().chain(page.strokes.iter()) {
            if rect.fill[3] == 0 {
                continue;
            }
            if is_text_decoration(rect) {
                decorations.push(rect);
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
                    docagent_layout::LINK_COLOR
                } else if line.code {
                    docagent_layout::CODE_SPAN_COLOR
                } else if line.strike {
                    docagent_layout::STRIKE_COLOR
                } else if line.color == [0, 0, 0, 255] {
                    docagent_layout::BODY_FG
                } else {
                    line.color
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
        for rect in decorations {
            ops.push(Op::FillRect {
                x: rect.x,
                y: rect.y,
                w: rect.width,
                h: rect.height,
                color: rect.fill,
            });
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
    fn paint_superscript_is_weasyprint_075em() {
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
            Op::Text { y, size, text, .. } if text.contains("PDF") => Some((*y, *size)),
            _ => None,
        });
        let raised = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, .. } if text == "A" => Some((*y, *size)),
            _ => None,
        });
        let (by, bsize) = base.expect("base");
        let (ry, rsize) = raised.expect("super");
        assert_eq!(
            rsize,
            docagent_layout::SUPER_SUB_SIZE_HU,
            "super size {rsize} must be letter CSS 0.75em, not missing"
        );
        assert_eq!(rsize, docagent_layout::super_sub_size(bsize));
        assert_eq!(
            ry,
            by.saturating_sub(docagent_layout::SUPER_SUB_SHIFT_HU),
            "super y {ry} vs {by} must rise WeasyPrint 0.5em"
        );
        assert_eq!(docagent_layout::SUPER_SUB_SIZE_HU, 750);
        assert_eq!(docagent_layout::SUPER_SUB_SHIFT_HU, 375);
    }

    #[test]
    fn paint_subscript_is_weasyprint_075em() {
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
            Op::Text { y, size, text, .. } if text == "H" => Some((*y, *size)),
            _ => None,
        });
        let lowered = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, .. } if text == "2" => Some((*y, *size)),
            _ => None,
        });
        let (by, bsize) = base.expect("base");
        let (ry, rsize) = lowered.expect("sub");
        assert_eq!(
            rsize,
            docagent_layout::SUPER_SUB_SIZE_HU,
            "sub size {rsize} must be letter CSS 0.75em, not missing"
        );
        assert_eq!(rsize, docagent_layout::super_sub_size(bsize));
        assert_eq!(
            ry,
            by.saturating_add(docagent_layout::SUPER_SUB_SHIFT_HU),
            "sub y {ry} vs {by} must drop WeasyPrint 0.5em, not 1/5em"
        );
        assert_eq!(docagent_layout::SUPER_SUB_SIZE_HU, 750);
        assert_eq!(docagent_layout::SUPER_SUB_SHIFT_HU, 375);
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
                        if text.contains("DocAgent") && *color == docagent_layout::LINK_COLOR
                )
            }),
            "link text must be teal: {:?}",
            list.ops
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
            "link underline fill missing: {:?}",
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
            "h1 paint size {h1_size} must be letter CSS 1.75rem"
        );
        assert_eq!(
            h1_color,
            docagent_layout::BODY_FG,
            "h1 must paint letter CSS #134e4a, not UA black"
        );
        assert_eq!(
            h2_size,
            docagent_layout::H2_SIZE_MAX_HU,
            "h2 paint size {h2_size} must be letter CSS 1.15rem"
        );
        assert_eq!(
            h2_color,
            docagent_layout::BODY_FG,
            "h2 must paint letter CSS #134e4a, not UA black"
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
                        if *color == docagent_layout::TH_BG && *w > 80 && *h > 80
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
            gap
                <= docagent_layout::H2_BEFORE_HU
                    + docagent_layout::H1_RULE_GAP_HU
                    + docagent_layout::H1_RULE_HU
                    + docagent_layout::H1_SIZE_MAX_HU
                    + docagent_layout::H2_SIZE_MAX_HU,
            "heading stack too airy: {gap}"
        );
        assert!(
            gap
                >= docagent_layout::H2_BEFORE_HU
                    + docagent_layout::H1_RULE_GAP_HU
                    + docagent_layout::H1_RULE_HU,
            "heading stack must keep letter CSS 1.1rem after the in-box 3px rule: {gap}"
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
        assert_eq!(docagent_layout::H1_RULE_HU, 225);
        assert_eq!(docagent_layout::H1_RULE_GAP_HU, 480);
    }

    #[test]
    fn paint_letter_h1_is_weasyprint_3px_04rem() {
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
        section.body.push(Block::Paragraph(h2));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let h1 = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, color, .. } if text.contains("Northwind") => {
                Some((*y, *size, *color))
            }
            _ => None,
        });
        let h2 = list.ops.iter().find_map(|op| match op {
            Op::Text { y, text, color, .. } if text.contains("Delivery") => Some((*y, *color)),
            _ => None,
        });
        let (h1y, h1size, h1color) = h1.expect("h1");
        let (h2y, h2color) = h2.expect("h2");
        assert_eq!(
            h1color,
            docagent_layout::BODY_FG,
            "h1 must paint letter CSS #134e4a, not UA black"
        );
        assert_eq!(
            h2color,
            docagent_layout::BODY_FG,
            "h2 must paint letter CSS #134e4a, not UA black"
        );
        assert_eq!(h1size, docagent_layout::H1_SIZE_MAX_HU);
        let rule = list.ops.iter().find_map(|op| match op {
            Op::FillRect { y, h, w, color, .. }
                if *color == docagent_layout::BODY_FG
                    && *h == docagent_layout::H1_RULE_HU
                    && *w > 1000 =>
            {
                Some(*y)
            }
            _ => None,
        });
        let ry = rule.expect("h1 3px rule");
        assert!(
            ry >= h1y,
            "h1 rule y {ry} must sit below baseline {h1y}"
        );
        assert!(
            h2y >= ry.saturating_add(docagent_layout::H1_RULE_HU),
            "h2 baseline {h2y} overlaps h1 3px rule {ry}"
        );
        let stack = h2y.saturating_sub(h1y);
        assert!(
            stack
                >= h1size
                    .saturating_add(docagent_layout::H1_RULE_GAP_HU)
                    .saturating_add(docagent_layout::H1_RULE_HU),
            "painted h1/h2 stack {stack} must include 0.4rem pad + 3px rule in the box"
        );
        assert_eq!(docagent_layout::H1_RULE_HU, 225);
        assert_eq!(docagent_layout::H1_RULE_GAP_HU, 480);
        assert_eq!(
            docagent_layout::H1_RULE_HU,
            docagent_layout::HR_THICK_HU
        );
    }

    #[test]
    fn paint_letter_h2_rule_is_weasyprint_full_content_width() {
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
        let tree = layout_document(&doc, &FontSet::bundled());
        let list = paint(&tree);
        let page_w = tree.pages[0].width;
        let content_w = page_w.saturating_sub(docagent_layout::PAGE_MARGIN_X_HU.saturating_mul(2));
        let h2 = list.ops.iter().find_map(|op| match op {
            Op::Text { x, text, color, .. } if text.contains("Delivery") => Some((*x, *color)),
            _ => None,
        });
        let (h2x, h2color) = h2.expect("h2");
        assert_eq!(h2color, docagent_layout::BODY_FG);
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
            "painted h2 border-bottom x {rx} must be letter CSS content left, not text x {h2x}"
        );
        assert_eq!(
            rw, content_w,
            "painted h2 rule width {rw} must be letter CSS block border-bottom, not text span"
        );
        assert_eq!(rw, h1w, "h2 border-bottom must span the same content box as h1");
        assert_eq!(h1x, docagent_layout::PAGE_MARGIN_X_HU);
        assert!(rw > 1000, "h2 rule must not collapse to a text-span hairline");
        assert_eq!(docagent_layout::H2_RULE_HU, 150);
        assert_eq!(docagent_layout::H2_RULE_GAP_HU, 240);
        assert_eq!(docagent_layout::TH_RULE, [13, 148, 136, 255]);
        assert_eq!(docagent_layout::PAGE_MARGIN_X_HU, 3300);
        assert_eq!(docagent_layout::H1_RULE_HU, 225);
        assert_eq!(docagent_layout::H1_AFTER_HU, 720);
        assert_eq!(docagent_layout::H2_BEFORE_HU, 1320);
        assert_eq!(docagent_layout::H2_AFTER_HU, 540);
        assert_eq!(docagent_layout::CELL_PAD_X_HU, 780);
    }

    #[test]
    fn paint_letter_h1_letter_spacing_is_weasyprint_neg_02em() {
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
            h1.width,
            h1_natural.saturating_add(docagent_layout::H1_LETTER_SPACING_HU.saturating_mul(gaps)),
            "h1 width {} must bake letter CSS -.02em, not UA normal {}",
            h1.width,
            h1_natural
        );
        assert!(h1.width < h1_natural);
        assert_eq!(
            h2.width, h2_natural,
            "letter.css letter-spacing is h1-only"
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
            "h1 must stay one Text run: {:?}",
            list.ops
        );
    }

    #[test]
    fn paint_letter_heading_line_height_is_weasyprint_12() {
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
        assert_eq!(h1.height, 2520, "h1 line box must stay letter CSS 1.2 of 1.75rem");
        assert_eq!(h2.height, 1656, "h2 line box must stay letter CSS 1.2 of 1.15rem");
        assert!(
            h1.height < h1_natural,
            "painted h1 1.2em {} must beat Noto natural {}",
            h1.height,
            h1_natural
        );
        assert!(
            h2.height < h2_natural,
            "painted h2 1.2em {} must beat Noto natural {}",
            h2.height,
            h2_natural
        );
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
        let natural_stack = h1_natural.saturating_add(h2_natural);
        assert!(
            stack < natural_stack,
            "painted h1/h2 stack {stack} must be tighter than Noto (asc+desc) {natural_stack}"
        );
        assert_eq!(docagent_layout::H1_SIZE_MAX_HU, 2100);
        assert_eq!(docagent_layout::H2_SIZE_MAX_HU, 1380);
        assert_eq!(docagent_layout::H2_BEFORE_HU, 1320);
    }

    #[test]
    fn paint_letter_table_cell_padding_is_weasyprint_px() {
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
        let (cx, cy, _, _) = cell.expect("header fill");
        let hop = list.ops.iter().find_map(|op| match op {
            Op::Text { x, y, text, .. } if text.contains("Hop") => Some((*x, *y)),
            _ => None,
        });
        let (tx, ty) = hop.expect("header text");
        assert_eq!(
            tx,
            cx.saturating_add(docagent_layout::CELL_PAD_X_HU),
            "painted cell pad x {} must be letter CSS 0.65rem",
            tx.saturating_sub(cx)
        );
        assert_eq!(
            hop_line.y,
            cy.saturating_add(docagent_layout::CELL_PAD_Y_HU),
            "header line box {} must sit on letter CSS 0.4rem pad",
            hop_line.y.saturating_sub(cy)
        );
        assert_eq!(
            ty,
            hop_line.y.saturating_add(hop_line.baseline),
            "painted th baseline {ty} must keep the 0.4rem cell pad under the type"
        );
        let hop_size = list.ops.iter().find_map(|op| match op {
            Op::Text { size, text, .. } if text.contains("Hop") => Some(*size),
            _ => None,
        });
        assert_eq!(
            hop_size.expect("th size"),
            docagent_layout::CELL_SIZE_HU,
            "painted th type must be letter CSS 0.95rem"
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
            "th 2px #0d9488 missing"
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
    }

    #[test]
    fn paint_letter_table_cell_valign_is_weasyprint_top() {
        fn hop_table(short: docagent_model::VAlign) -> docagent_model::Table {
            let mut table = docagent_model::Table::from_cells(vec![
                vec!["Hop".into(), "File".into()],
                vec![
                    "The shipment left Busan on 4 September and is booked on the 11 September rail window. This is a flow document: one convert, three hashes, GPU-free.".into(),
                    "Plan".into(),
                ],
            ]);
            table.header_row_count = 1;
            table.rows[1].cells[1].valign = short;
            table
        }
        let mut top_doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Table(hop_table(docagent_model::VAlign::Top)));
        top_doc.sections.push(section);
        let top = paint(&layout_document(&top_doc, &FontSet::bundled()));
        let wrap_y = top.ops.iter().find_map(|op| match op {
            Op::Text { y, text, .. }
                if text.contains("Busan") || text.contains("shipment") =>
            {
                Some(*y)
            }
            _ => None,
        });
        let plan_y = top.ops.iter().find_map(|op| match op {
            Op::Text { y, text, .. } if text.contains("Plan") => Some(*y),
            _ => None,
        });
        let wy = wrap_y.expect("wrapping hop cell");
        let py = plan_y.expect("short hop cell");
        assert_eq!(
            py, wy,
            "painted short hop cell must stay letter CSS vertical-align:top, not UA middle"
        );

        let mut mid_doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Table(hop_table(docagent_model::VAlign::Middle)));
        mid_doc.sections.push(section);
        let mid = paint(&layout_document(&mid_doc, &FontSet::bundled()));
        let mid_plan = mid.ops.iter().find_map(|op| match op {
            Op::Text { y, text, .. } if text.contains("Plan") => Some(*y),
            _ => None,
        });
        let mid_wrap = mid.ops.iter().find_map(|op| match op {
            Op::Text { y, text, .. }
                if text.contains("Busan") || text.contains("shipment") =>
            {
                Some(*y)
            }
            _ => None,
        });
        let my = mid_plan.expect("UA middle short cell");
        let mw = mid_wrap.expect("UA middle wrap");
        assert!(
            my > mw,
            "painted UA middle {my} must sit below wrapping top {mw}"
        );
        assert!(
            my > py,
            "painted letter CSS top {py} must beat UA middle {my}"
        );
        assert_eq!(docagent_layout::CELL_PAD_Y_HU, 480);
        assert_eq!(docagent_layout::CELL_PAD_X_HU, 780);
        assert_eq!(docagent_layout::CELL_SIZE_HU, 1140);
        assert_eq!(
            docagent_layout::cell_valign_offset(docagent_model::VAlign::Top, 2000, 5000),
            0
        );
        assert_eq!(
            docagent_layout::cell_valign_offset(docagent_model::VAlign::Middle, 2000, 5000),
            1500
        );
    }

    #[test]
    fn paint_letter_table_grid_is_weasyprint_collapse() {
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
            "painted internal vertical must collapse to one 1px, not two adjacent 1px boxes"
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
            "painted header/body seam must stay 2px #0d9488, not a stacked 1px gray"
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
            "painted internal horizontal must collapse to one 1px, not two stacked 1px boxes"
        );
        assert_eq!(docagent_layout::CELL_BORDER_HU, 75);
        assert_eq!(
            docagent_layout::TH_RULE_HU,
            2 * docagent_layout::CELL_BORDER_HU
        );
    }

    #[test]
    fn paint_letter_table_header_is_weasyprint_th() {
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
        assert_eq!(th_fills, 3, "letter th fill missing: {:?}", list.ops);
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
        assert!(
            list.ops.iter().all(|op| {
                !matches!(op, Op::FillRect { color, .. } if *color == [0, 0, 0, 255])
            }),
            "letter table must not keep a sparse black grid"
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
        assert!(th_rules >= 3, "th 2px #0d9488 missing: {th_rules}");
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
        let strike_i = list.ops.iter().position(|op| {
            matches!(
                op,
                Op::FillRect { x, y, h, color, .. }
                    if *color == docagent_layout::STRIKE_COLOR
                        && *h == docagent_layout::STRIKE_HU
                        && *x == gx
                        && *y == gy.saturating_sub(gsize / 3)
            )
        });
        let guess_i = list.ops.iter().position(|op| {
            matches!(op, Op::Text { text, .. } if text.contains("guess"))
        });
        assert!(
            strike_i.expect("strike fill") > guess_i.expect("strike glyphs"),
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
        assert_eq!(docagent_layout::TH_FG, [19, 78, 74, 255]);
        assert_eq!(docagent_layout::CELL_BORDER_HU, 75);
        assert_eq!(docagent_layout::TH_RULE_HU, 150);
        assert_eq!(docagent_layout::TASK_BOX_HU, 900);
        assert_eq!(docagent_layout::TASK_BORDER_HU, 150);
        assert_eq!(docagent_layout::LIST_MARK_GAP_HU, 480);
        assert_eq!(docagent_layout::CELL_PAD_X_HU, 780);
        assert_eq!(docagent_layout::CELL_SIZE_HU, 1140);
        assert_eq!(docagent_layout::IMAGE_BEFORE_HU, 720);
        assert_eq!(docagent_layout::H1_SIZE_MAX_HU, 2100);
        assert_eq!(docagent_layout::H2_SIZE_MAX_HU, 1380);
        assert_eq!(docagent_layout::HR_THICK_HU, 225);
        assert_eq!(docagent_layout::LINK_COLOR, [13, 148, 136, 255]);
        assert_eq!(docagent_layout::CODE_SPAN_PAD_X_HU, 350);
        assert_eq!(docagent_layout::MARK_PAD_X_HU, 200);
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
        let pdf = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, .. } if text.contains("PDF") => Some((*y, *size)),
            _ => None,
        });
        let raised = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, .. } if text == "A" => Some((*y, *size)),
            _ => None,
        });
        let (py, psize) = pdf.expect("PDF");
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
    }

    #[test]
    fn paint_letter_gap_after_mark_ignores_body_after_space() {
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
            "line box after mark must be letter CSS 0.7rem"
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
            "type after the 24pt mark must stay letter CSS 10pt body"
        );
        assert_eq!(
            by,
            date_line.y.saturating_add(date_line.baseline),
            "painted date baseline must keep the 0.7rem gap under the type"
        );
        let glyph_top = by.saturating_sub(bsize);
        assert!(
            glyph_top >= iy.saturating_add(ih),
            "date overlaps mark"
        );
        let gap = glyph_top.saturating_sub(iy.saturating_add(ih));
        assert!(
            gap >= docagent_layout::IMAGE_AFTER_HU,
            "gap after mark {gap} must be letter CSS 0.7rem, not jammed"
        );
        assert!(
            gap < docagent_layout::IMAGE_AFTER_HU.saturating_add(400),
            "gap after mark {gap} must not keep body space_after 200"
        );
    }

    #[test]
    fn paint_letter_gap_before_mark_under_h2_is_weasyprint_06rem() {
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
        let (h2y, h2size, h2color) = h2.expect("h2");
        assert_eq!(h2color, docagent_layout::BODY_FG);
        assert_eq!(h2size, docagent_layout::H2_SIZE_MAX_HU);
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
        assert!(
            iy >= ry.saturating_add(docagent_layout::H2_RULE_HU),
            "mark overlaps h2 rule"
        );
        let after_rule = iy.saturating_sub(ry.saturating_add(docagent_layout::H2_RULE_HU));
        assert!(
            after_rule >= docagent_layout::IMAGE_BEFORE_HU,
            "gap after h2 rule {after_rule} must be letter CSS 0.6rem, not jammed under H2"
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
            "gap after mark {gap} must stay letter CSS 0.7rem"
        );
        let _ = h2y;
        assert_eq!(docagent_layout::IMAGE_BEFORE_HU, 720);
        assert_eq!(docagent_layout::H2_RULE_HU, 150);
        assert_eq!(docagent_layout::H2_RULE_GAP_HU, 240);
        assert_eq!(docagent_layout::H2_AFTER_HU, 540);
        assert_eq!(docagent_layout::CELL_SIZE_HU, 1140);
    }

    #[test]
    fn paint_letter_first_line_after_h2_is_weasyprint_045rem() {
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
        let (h2y, h2size, h2color) = h2.expect("h2");
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
            "PDF/paint first line after h2 {after_rule} must be letter CSS 0.45rem, not IR 240"
        );
        assert!(
            after_rule < docagent_layout::H2_AFTER_HU.saturating_add(400),
            "body after h2 {after_rule} must stay 0.45rem, not the 0.6rem 24pt-mark gap"
        );
        let _ = h2y;
        assert_eq!(docagent_layout::H2_AFTER_HU, 540);
        assert_eq!(docagent_layout::H2_BEFORE_HU, 1320);
        assert_eq!(docagent_layout::H1_AFTER_HU, 720);
        assert_eq!(docagent_layout::IMAGE_BEFORE_HU, 720);
        assert_eq!(docagent_layout::H1_RULE_HU, 225);
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
    fn paint_letter_list_item_gap_is_weasyprint_035rem() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Paragraph(bullet_para("Hash the input", 0)));
        section.body.push(Block::Paragraph(bullet_para("Run Convert", 0)));
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
        let (y1, s1) = first.expect("first");
        let y2 = second.expect("second");
        let step = y2.saturating_sub(y1);
        assert!(
            step >= s1.saturating_add(docagent_layout::LIST_ITEM_AFTER_HU),
            "painted list step {step} must include letter CSS 0.35rem"
        );
        assert!(
            step <= s1.saturating_mul(2).saturating_add(docagent_layout::LIST_ITEM_AFTER_HU),
            "painted list step {step} too sparse vs letter CSS 0.35rem"
        );
        const {
            assert!(docagent_layout::LIST_ITEM_AFTER_HU < docagent_layout::BODY_AFTER_HU)
        };
    }

    #[test]
    fn paint_letter_list_indent_is_weasyprint_125rem() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(bullet_para("Receiving dock is clear", 0)));
        section
            .body
            .push(Block::Paragraph(bullet_para("Bay 4 seals intact", 1)));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let dock = list.ops.iter().find_map(|op| match op {
            Op::Text { x, text, .. } if text.contains("dock") => Some(*x),
            _ => None,
        });
        let bay = list.ops.iter().find_map(|op| match op {
            Op::Text { x, text, .. } if text.contains("Bay") => Some(*x),
            _ => None,
        });
        let dock_x = dock.expect("dock");
        let bay_x = bay.expect("bay");
        assert_eq!(
            bay_x.saturating_sub(dock_x),
            docagent_layout::LIST_INDENT_HU,
            "nested hang {} must be letter CSS 1.25rem",
            bay_x.saturating_sub(dock_x)
        );
    }

    #[test]
    fn paint_letter_page_margin_is_weasyprint_25rem_275rem() {
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
            "painted body x {x} must be letter CSS @page 2.75rem, not Hangul 30mm"
        );
        assert!(
            (docagent_layout::PAGE_MARGIN_Y_HU..docagent_model::DEFAULT_MARGIN_HU).contains(&y),
            "painted baseline y {y} must sit in letter CSS @page 2.5rem, not Hangul 30mm"
        );
        assert_eq!(docagent_layout::PAGE_MARGIN_X_HU, 3300);
        assert_eq!(docagent_layout::PAGE_MARGIN_Y_HU, 3000);
        assert_ne!(
            docagent_layout::PAGE_MARGIN_X_HU,
            docagent_model::DEFAULT_MARGIN_HU
        );
        assert_ne!(
            docagent_layout::PAGE_MARGIN_X_HU,
            75 * docagent_model::HU_PER_INCH / 96,
            "WeasyPrint UA @page margin 75px is not letter.html"
        );
        const {
            assert!(docagent_layout::PAGE_MARGIN_X_HU < docagent_model::DEFAULT_MARGIN_HU)
        };
    }

    #[test]
    fn paint_letter_nested_list_gap_is_weasyprint_025rem() {
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
            "painted nested step {nested_step} must be 0.10rem tighter than sibling {sibling_step}"
        );
        assert!(
            nested_step >= s1.saturating_add(docagent_layout::LIST_NESTED_GAP_HU),
            "painted nested step {nested_step} must include letter CSS 0.25rem"
        );
        assert!(
            nested_step < sibling_step,
            "nested 0.25rem must read tighter than sibling 0.35rem"
        );
        assert_eq!(docagent_layout::LIST_NESTED_GAP_HU, 300);
        assert_eq!(docagent_layout::LIST_ITEM_AFTER_HU, 420);
        assert_eq!(docagent_layout::LIST_INDENT_HU, 1500);
        const {
            assert!(docagent_layout::LIST_NESTED_GAP_HU < docagent_layout::LIST_ITEM_AFTER_HU)
        };
    }

    #[test]
    fn paint_letter_quote_spacing_is_weasyprint_blockquote() {
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
        let mut after = Paragraph::from_text("Please confirm before 09:00 local.");
        after.space_after = 200;
        section.body.push(Block::Paragraph(after));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let before = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, .. } if text.contains("Wet cargo") => Some((*y, *size)),
            _ => None,
        });
        let quote = list.ops.iter().find_map(|op| match op {
            Op::Text { x, y, size, text, .. } if text.contains("Replay") => {
                Some((*x, *y, *size))
            }
            _ => None,
        });
        let after = list.ops.iter().find_map(|op| match op {
            Op::Text { y, text, .. } if text.contains("confirm") => Some(*y),
            _ => None,
        });
        let (y0, s0) = before.expect("before");
        let (qx, qy, qs) = quote.expect("quote");
        let y2 = after.expect("after");
        let into_quote = qy.saturating_sub(y0);
        assert!(
            into_quote
                >= s0
                    .saturating_add(docagent_layout::BODY_AFTER_HU.max(docagent_layout::QUOTE_BEFORE_HU))
                    .saturating_add(docagent_layout::QUOTE_PAD_Y_HU),
            "quote step {into_quote} jammed vs letter CSS blockquote"
        );
        let after_quote = y2.saturating_sub(qy);
        assert!(
            after_quote
                >= qs
                    .saturating_add(docagent_layout::QUOTE_PAD_Y_HU)
                    .saturating_add(docagent_layout::QUOTE_AFTER_HU),
            "after quote {after_quote} jammed vs letter CSS 1rem"
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
            "quote bar width must be letter CSS 4px"
        );
        assert!(
            qx > docagent_layout::QUOTE_PAD_X_HU,
            "quote text x {qx} must include 1rem pad"
        );
        assert_eq!(docagent_layout::QUOTE_FG, [19, 78, 74, 255]);
        assert_eq!(docagent_layout::QUOTE_BAR_W_HU, 300);
    }

    #[test]
    fn paint_letter_code_block_spacing_is_weasyprint_pre() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut before = Paragraph::from_text("Replay is evidence.");
        before.space_after = 200;
        section.body.push(Block::Paragraph(before));
        let mut code = Paragraph::from_text("docagent prove examples/letter.md");
        code.code_block = true;
        code.space_before = 80;
        code.space_after = 200;
        section.body.push(Block::Paragraph(code));
        let mut after = Paragraph::from_text("Hop File Proof");
        after.space_after = 200;
        section.body.push(Block::Paragraph(after));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let before = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, .. } if text.contains("Replay") => Some((*y, *size)),
            _ => None,
        });
        let code = list.ops.iter().find_map(|op| match op {
            Op::Text { x, y, size, text, .. } if text.contains("docagent prove") => {
                Some((*x, *y, *size))
            }
            _ => None,
        });
        let after = list.ops.iter().find_map(|op| match op {
            Op::Text { y, text, .. } if text.contains("Hop") => Some(*y),
            _ => None,
        });
        let (y0, s0) = before.expect("before");
        let (cx, cy, cs) = code.expect("code");
        let y2 = after.expect("after");
        let into_code = cy.saturating_sub(y0);
        assert!(
            into_code
                >= s0
                    .saturating_add(docagent_layout::BODY_AFTER_HU.max(docagent_layout::CODE_BEFORE_HU))
                    .saturating_add(docagent_layout::CODE_PAD_Y_HU),
            "code step {into_code} jammed vs letter CSS pre"
        );
        let after_code = y2.saturating_sub(cy);
        assert!(
            after_code
                >= cs
                    .saturating_add(docagent_layout::CODE_PAD_Y_HU)
                    .saturating_add(docagent_layout::CODE_AFTER_HU),
            "after code {after_code} jammed vs letter CSS 1rem"
        );
        assert!(
            cx > docagent_layout::CODE_PAD_X_HU,
            "code text x {cx} must include 1rem pad"
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::FillRect { color, w, h, .. }
                        if *color == [204, 251, 241, 255]
                            && *w > 10000
                            && *h > docagent_layout::CODE_PAD_Y_HU
                )
            }),
            "code fill must include 0.75rem pad"
        );
    }

    #[test]
    fn paint_letter_code_block_pad_x_is_weasyprint_1rem_both_sides() {
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
            "painted pre must wrap so right pad is observable"
        );
        for line in &lines {
            assert_eq!(
                line.x,
                origin.saturating_add(docagent_layout::CODE_PAD_X_HU),
                "painted code x {} must stay letter CSS 1rem left pad",
                line.x.saturating_sub(origin)
            );
            assert!(
                line.x.saturating_add(line.width) <= right_limit,
                "painted code right {} must keep letter CSS 1rem right pad",
                line.x.saturating_add(line.width)
            );
        }
        let texts: Vec<(i32, &str)> = list
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::Text { x, text, .. } if !text.is_empty() => Some((*x, text.as_str())),
                _ => None,
            })
            .collect();
        assert!(texts.len() >= 2, "paint must emit wrapped pre runs: {texts:?}");
        assert!(
            texts
                .iter()
                .all(|(x, _)| *x == origin.saturating_add(docagent_layout::CODE_PAD_X_HU)),
            "painted pre glyphs must sit on the 1rem left pad: {texts:?}"
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
        assert_eq!(fx, origin, "painted pre background is the padding box");
        assert_eq!(
            fw, content_w,
            "painted pre fill {fw} must span the content box, not shrink by 1rem"
        );
        assert_eq!(docagent_layout::CODE_PAD_X_HU, 1200);
        assert_eq!(docagent_layout::CODE_PAD_Y_HU, 900);
    }

    #[test]
    fn paint_letter_inline_code_is_weasyprint_092em() {
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
            Op::Text { size, text, .. } if text.contains("Run") => Some(*size),
            _ => None,
        });
        let prove = list.ops.iter().find_map(|op| match op {
            Op::Text { size, text, .. } if text.contains("prove") && *size < docagent_model::DEFAULT_FONT_SIZE_HU => {
                Some(*size)
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
        let rsize = run.expect("run");
        let psize = prove.expect("prove");
        let tsize = twice.expect("twice");
        let csize = pre_size.expect("pre");
        assert_eq!(rsize, docagent_model::DEFAULT_FONT_SIZE_HU);
        assert_eq!(tsize, docagent_model::DEFAULT_FONT_SIZE_HU);
        assert_eq!(
            psize,
            docagent_layout::CODE_SPAN_SIZE_HU,
            "painted code size {psize} must be letter CSS 0.92em, not body 10pt"
        );
        assert_eq!(psize, docagent_layout::code_span_size(rsize));
        assert_eq!(
            csize,
            docagent_layout::CODE_SPAN_SIZE_HU,
            "painted pre code size {csize} must inherit letter CSS 0.92em"
        );
        assert_eq!(docagent_layout::CODE_SPAN_SIZE_HU, 920);
        assert_eq!(docagent_layout::CODE_SPAN_PAD_X_HU, 350);
        assert_eq!(docagent_layout::H2_AFTER_HU, 540);
        assert_eq!(docagent_layout::CELL_PAD_X_HU, 780);
    }

    #[test]
    fn paint_letter_body_paragraph_gap_is_weasyprint_07rem() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut date = Paragraph::from_text("6 September 2026");
        date.space_after = 200;
        section.body.push(Block::Paragraph(date));
        let mut dest = Paragraph::from_text("To: operations desk");
        dest.space_after = 200;
        section.body.push(Block::Paragraph(dest));
        doc.sections.push(section);
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let first = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, .. } if text.contains("September") => Some((*y, *size)),
            _ => None,
        });
        let second = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, .. } if text.contains("operations") => Some((*y, *size)),
            _ => None,
        });
        let (y1, s1) = first.expect("date");
        let (y2, _s2) = second.expect("dest");
        let step = y2.saturating_sub(y1);
        assert!(
            step >= s1.saturating_add(docagent_layout::BODY_AFTER_HU),
            "painted body step {step} must include letter CSS 0.7rem after, not IR 200"
        );
        assert!(
            step <= s1.saturating_mul(2).saturating_add(docagent_layout::BODY_AFTER_HU),
            "painted body step {step} too sparse"
        );
    }

    #[test]
    fn paint_letter_hr_is_weasyprint_3px_11rem() {
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
            Op::Text { y, size, text, .. } if text.contains("SHA-256") => Some((*y, *size)),
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
        let (y0, s0) = before.expect("before");
        let (ry, rh) = rule.expect("hr");
        let y2 = after.expect("after");
        assert_eq!(rh, docagent_layout::HR_THICK_HU, "hr must stay letter CSS 3px");
        assert!(
            ry >= y0.saturating_add(docagent_layout::BODY_AFTER_HU.max(docagent_layout::HR_BEFORE_HU)),
            "hr y {ry} jammed vs letter CSS 1.1rem after baseline {y0} size {s0}"
        );
        assert!(
            y2 >= ry
                .saturating_add(docagent_layout::HR_THICK_HU)
                .saturating_add(docagent_layout::HR_AFTER_HU),
            "after hr {y2} jammed vs letter CSS 1.1rem"
        );
    }

    #[test]
    fn paint_letter_link_is_weasyprint_teal_underline() {
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
            "link glyphs must be letter CSS #0d9488: {:?}",
            list.ops
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Text { text, color, .. }
                        if text.contains("runtime") && *color == docagent_layout::BODY_FG
                )
            }),
            "following text must be letter body #134e4a, not UA black"
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
            "link underline must paint WeasyPrint teal: {:?}",
            list.ops
        );
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Link { uri, w, h, .. }
                        if uri == "https://github.com/kevin9327/docagent" && *w > 0 && *h > 0
                )
            }),
            "link op missing"
        );
    }

    #[test]
    fn paint_letter_strike_and_underline_are_weasyprint_s_u() {
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
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Text { text, color, .. }
                        if text.contains("Agents") && *color == docagent_layout::BODY_FG
                )
            }),
            "plain text must be letter body #134e4a, not UA black"
        );
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
                    Op::FillRect { x, y, w, h, color }
                        if *color == docagent_layout::STRIKE_COLOR
                            && *h == docagent_layout::STRIKE_HU
                            && *x == gx
                            && *w > 80
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
            "em must not regress"
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
        assert_eq!(docagent_layout::TH_BG, [204, 251, 241, 255]);
        assert_eq!(docagent_layout::TASK_BOX_HU, 900);
        assert_eq!(docagent_layout::HR_THICK_HU, 225);
        assert_eq!(docagent_layout::LINK_COLOR, [13, 148, 136, 255]);
        assert_eq!(docagent_layout::CODE_SPAN_PAD_X_HU, 350);
        assert_eq!(docagent_layout::MARK_PAD_X_HU, 200);
        let pdf = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, .. } if text.contains("PDF") => Some((*y, *size)),
            _ => None,
        });
        let raised = list.ops.iter().find_map(|op| match op {
            Op::Text { y, size, text, .. } if text == "A" => Some((*y, *size)),
            _ => None,
        });
        let (py, psize) = pdf.expect("PDF");
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
    }

    #[test]
    fn paint_letter_inline_code_is_weasyprint_code() {
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
        let run = list.ops.iter().find_map(|op| match op {
            Op::Text { x, text, .. } if text.contains("Run") => Some(*x),
            _ => None,
        });
        let twice = list.ops.iter().find_map(|op| match op {
            Op::Text { x, text, color, .. } if text.contains("twice") => Some((*x, *color)),
            _ => None,
        });
        let (px, py, psize, pcolor) = prove.expect("prove");
        let run_x = run.expect("run");
        let (tx, tcolor) = twice.expect("twice");
        assert_eq!(pcolor, docagent_layout::CODE_SPAN_COLOR);
        assert_eq!(
            tcolor,
            docagent_layout::BODY_FG,
            "body beside code must be letter #134e4a, not UA black"
        );
        assert!(px > run_x);
        assert!(tx > px);
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
    }

    #[test]
    fn paint_letter_kbd_samp_are_weasyprint_code_chip() {
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
            Op::Text { x, y, size, text, color, .. } if text.contains("SHA-256") => {
                Some((*x, *y, *size, *color))
            }
            _ => None,
        });
        let run = list.ops.iter().find_map(|op| match op {
            Op::Text { x, size, text, color, .. } if text.contains("Run") => {
                Some((*x, *size, *color))
            }
            _ => None,
        });
        let (px, py, psize, pcolor) = prove.expect("kbd");
        let (hx, hy, hsize, hcolor) = hash.expect("samp");
        let (rx, rsize, rcolor) = run.expect("run");
        assert_eq!(pcolor, docagent_layout::CODE_SPAN_COLOR);
        assert_eq!(hcolor, docagent_layout::CODE_SPAN_COLOR);
        assert_eq!(
            rcolor,
            docagent_layout::BODY_FG,
            "body beside kbd/samp must be letter #134e4a, not UA black"
        );
        assert_eq!(psize, docagent_layout::CODE_SPAN_SIZE_HU);
        assert_eq!(hsize, docagent_layout::CODE_SPAN_SIZE_HU);
        assert_eq!(rsize, docagent_model::DEFAULT_FONT_SIZE_HU);
        assert!(px > rx);
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
            "kbd and samp must each paint a mint chip: {:?}",
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
        let (sx, sy, sw, sh) = fills[1];
        assert_eq!(sx, hx.saturating_sub(docagent_layout::CODE_SPAN_PAD_X_HU));
        assert_eq!(
            sy,
            hy.saturating_sub(hsize)
                .saturating_sub(docagent_layout::CODE_SPAN_PAD_Y_HU)
        );
        assert_eq!(
            sh,
            hsize.saturating_add(docagent_layout::CODE_SPAN_PAD_Y_HU.saturating_mul(2))
        );
        assert!(sw > hsize);
        assert!(
            !list.ops.iter().any(|op| matches!(
                op,
                Op::FillRect { w, h, color, .. }
                    if (*w == docagent_layout::CELL_BORDER_HU
                        || *h == docagent_layout::CELL_BORDER_HU)
                        && *color != docagent_layout::CODE_SPAN_BG
            )),
            "painted kbd/samp must not keep WeasyPrint UA 1px border: {:?}",
            list.ops
        );
        assert_eq!(docagent_layout::CODE_SPAN_PAD_X_HU, 350);
        assert_eq!(docagent_layout::CODE_SPAN_PAD_Y_HU, 100);
        assert_eq!(docagent_layout::CODE_SPAN_SIZE_HU, 920);
        assert_eq!(docagent_layout::CODE_PAD_X_HU, 1200);
        assert_eq!(docagent_layout::H1_SIZE_MAX_HU, 2100);
        assert_eq!(docagent_layout::H2_AFTER_HU, 540);
        assert_eq!(docagent_layout::TASK_MARK_ADVANCE_HU, 1380);
        assert_eq!(docagent_layout::LIST_NESTED_GAP_HU, 300);
        assert_eq!(docagent_layout::PAGE_MARGIN_X_HU, 3300);
        assert_eq!(docagent_layout::QUOTE_BAR_W_HU, 300);
    }

    #[test]
    fn paint_letter_abbr_underline_is_weasyprint_dotted_teal() {
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
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let pdfa = list.ops.iter().find_map(|op| match op {
            Op::Text { x, y, size, text, color, .. } if text.contains("PDF/A") => {
                Some((*x, *y, *size, *color))
            }
            _ => None,
        });
        let local = list.ops.iter().find_map(|op| match op {
            Op::Text { x, y, text, color, .. } if text.contains("09:00") => {
                Some((*x, *y, *color))
            }
            _ => None,
        });
        let (ax, ay, asize, acolor) = pdfa.expect("abbr");
        let (ux, uy, ucolor) = local.expect("u");
        assert_eq!(acolor, docagent_layout::BODY_FG);
        assert_eq!(ucolor, docagent_layout::BODY_FG);
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
            "painted abbr must be 1px on/off dots, not a solid UA underline: {:?}",
            list.ops
        );
        assert_eq!(dots[0].0, ax);
        assert!(
            dots.iter().all(|(_, w)| *w <= docagent_layout::DOTTED_UNDERLINE_DOT_HU),
            "painted abbr dots must stay 1px: {dots:?}"
        );
        let abbr_i = list
            .ops
            .iter()
            .position(|op| matches!(op, Op::Text { text, .. } if text.contains("PDF/A")))
            .expect("abbr glyphs");
        let first_dot_i = list
            .ops
            .iter()
            .position(|op| {
                matches!(
                    op,
                    Op::FillRect { x, y, h, color, .. }
                        if *color == docagent_layout::BODY_FG
                            && *h == docagent_layout::UNDERLINE_HU
                            && *x == ax
                            && *y == ay.saturating_add(offset)
                )
            })
            .expect("abbr first dot");
        assert!(
            first_dot_i > abbr_i,
            "abbr dots must paint after glyphs like WeasyPrint, not missing under the ink"
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
            "painted abbr must not keep a solid underline: {:?}",
            list.ops
        );
        assert!(
            list.ops.iter().any(|op| matches!(
                op,
                Op::FillRect { x, y, h, color, .. }
                    if *color == docagent_layout::STRIKE_COLOR
                        && *h == docagent_layout::UNDERLINE_HU
                        && *x == ux
                        && *y == uy.saturating_add(docagent_layout::UNDERLINE_OFFSET_HU)
            )),
            "solid u must not regress"
        );
        assert_eq!(docagent_layout::ABBR_UNDERLINE_OFFSET_HU, 150);
        assert_eq!(docagent_layout::DOTTED_UNDERLINE_DOT_HU, 80);
        assert_eq!(docagent_layout::UNDERLINE_OFFSET_HU, 100);
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
    fn paint_letter_list_marker_is_weasyprint_teal() {
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
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
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
            "painted ol ::marker must be letter CSS #134e4a, not UA CanvasText black"
        );
        assert_eq!(two.expect("2. marker"), docagent_layout::MARKER_FG);
        assert_eq!(three.expect("3. marker"), docagent_layout::MARKER_FG);
        assert_ne!(one.expect("1. marker"), [0, 0, 0, 255]);
        assert_eq!(
            hash.expect("hash item"),
            docagent_layout::BODY_FG,
            "ol item text must stay letter body teal"
        );
        assert_eq!(date.expect("body"), docagent_layout::BODY_FG);
        assert!(
            list.ops.iter().any(|op| matches!(
                op,
                Op::FillRect { color, w, h, .. }
                    if *color == docagent_layout::MARKER_FG && *w == *h && *w >= 400
            )),
            "painted nested disc ::marker must be letter #134e4a: {:?}",
            list.ops
        );
        assert!(
            !list.ops.iter().any(|op| matches!(
                op,
                Op::FillRect { color, w, h, .. }
                    if *color == [0, 0, 0, 255] && *w == *h && *w >= 400
            )),
            "painted nested disc must not stay UA CanvasText black"
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
    fn paint_letter_pre_code_is_weasyprint_transparent_zero_pad() {
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
        let tree = layout_document(&doc, &FontSet::bundled());
        let list = paint(&tree);
        let origin = docagent_layout::PAGE_MARGIN_X_HU;
        let chip = list.ops.iter().find_map(|op| match op {
            Op::Text { x, text, .. } if *text == "prove" => Some(*x),
            _ => None,
        });
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
        let cx = chip.expect("inline prove");
        let (fx, fsize, fcolor) = fence.expect("pre code");
        assert_eq!(fsize, docagent_layout::CODE_SPAN_SIZE_HU);
        assert_eq!(fcolor, docagent_layout::CODE_SPAN_COLOR);
        assert_eq!(fcolor, docagent_layout::BODY_FG);
        assert_eq!(
            fx,
            origin.saturating_add(docagent_layout::CODE_PAD_X_HU),
            "painted pre code x {fx} must stay letter CSS 1rem pre pad, not 1rem+0.35em chip inset"
        );
        assert_ne!(
            fx,
            origin
                .saturating_add(docagent_layout::CODE_PAD_X_HU)
                .saturating_add(docagent_layout::CODE_SPAN_PAD_X_HU),
            "inherited code-span 0.35em must not inset the painted prove fence"
        );
        assert!(
            cx != fx,
            "inline chip and pre fence must not share an x"
        );
        assert!(docagent_layout::code_span_chip(true, false));
        assert!(!docagent_layout::code_span_chip(true, true));
        let pre_fill = list.ops.iter().any(|op| {
            matches!(
                op,
                Op::FillRect { color, w, h, .. }
                    if *color == docagent_layout::CODE_SPAN_BG
                        && *w > 10000
                        && *h >= docagent_layout::CODE_PAD_Y_HU.saturating_mul(2)
            )
        });
        assert!(pre_fill, "painted pre fill missing: {:?}", list.ops);
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
            "painted pre code must not keep an inner mint chip: {:?}",
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
    }

    #[test]
    fn paint_letter_mark_is_weasyprint_highlight() {
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
        let before = list.ops.iter().find_map(|op| match op {
            Op::Text { x, text, .. } if text.contains("convert") => Some(*x),
            _ => None,
        });
        let after = list.ops.iter().find_map(|op| match op {
            Op::Text { x, text, .. } if text.contains("GPU-free") => Some(*x),
            _ => None,
        });
        let (hx, hy, hsize, hcolor) = hashes.expect("hashes");
        let bx = before.expect("before");
        let ax = after.expect("after");
        assert_eq!(
            hcolor,
            docagent_layout::BODY_FG,
            "mark text must be letter body #134e4a, not UA black"
        );
        assert!(hx > bx);
        assert!(ax > hx);
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
        assert!(
            fh > hsize,
            "mark fill height {fh} must beat glyph size {hsize}"
        );
        assert_eq!(docagent_layout::MARK_PAD_X_HU, 200);
        assert_eq!(docagent_layout::CELL_PAD_X_HU, 780);
        assert_eq!(docagent_layout::H2_SIZE_MAX_HU, 1380);
        assert_eq!(docagent_layout::HR_BEFORE_HU, 1320);
        assert_eq!(docagent_layout::LINK_UNDERLINE_HU, 80);
        assert_eq!(docagent_layout::QUOTE_PAD_X_HU, 1200);
    }

    #[test]
    fn paint_letter_task_checkbox_is_weasyprint_box() {
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
        let dock = list.ops.iter().find_map(|op| match op {
            Op::Text { x, y, text, .. } if text.contains("dock") => Some((*x, *y)),
            _ => None,
        });
        let replay = list.ops.iter().find_map(|op| match op {
            Op::Text { x, y, text, .. } if text.contains("Replay") => Some((*x, *y)),
            _ => None,
        });
        let (dx, dy) = dock.expect("checked text");
        let (rx, ry) = replay.expect("unchecked text");
        let teal = [19, 78, 74, 255];
        let box_hu = docagent_layout::TASK_BOX_HU;
        let border_hu = docagent_layout::TASK_BORDER_HU;
        let gap_hu = docagent_layout::LIST_MARK_GAP_HU;
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
            "painted task checkbox x must be letter CSS ul.tasks padding-left 1.25rem, not a hanging marker"
        );
        assert_eq!(
            dx,
            cx.saturating_add(box_hu).saturating_add(gap_hu),
            "checked text gap must be letter CSS 0.4rem"
        );
        assert_eq!(
            dx,
            cx.saturating_add(docagent_layout::TASK_MARK_ADVANCE_HU),
            "painted task text must sit 0.9em + 0.4rem after the in-flow box"
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
        assert_eq!(horiz, 2, "unchecked - [ ] must paint 1.5pt top/bottom: {:?}", list.ops);
        assert_eq!(vert, 2, "unchecked - [ ] must paint 1.5pt left/right: {:?}", list.ops);
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
            "unchecked text gap must be letter CSS 0.4rem"
        );
        assert!(
            list.ops.iter().all(|op| {
                !matches!(op, Op::Text { text, .. } if text.contains('•'))
            }),
            "task lists must drop the disc: {:?}",
            list.ops
        );
        let step = ry.saturating_sub(dy);
        assert!(
            step >= docagent_layout::LIST_ITEM_AFTER_HU,
            "painted task step {step} must keep letter CSS 0.35rem"
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
        assert_eq!(docagent_layout::QUOTE_BAR_W_HU, 300);
        assert_eq!(docagent_layout::QUOTE_FG, [19, 78, 74, 255]);
    }

    #[test]
    fn paint_letter_body_quote_th_tasks_and_24pt_still_emit() {
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
        let list = paint(&layout_document(&doc, &FontSet::bundled()));
        let image = list.ops.iter().find_map(|op| match op {
            Op::Image { w, h, bytes, mime, y, .. }
                if bytes.as_slice() == MARK_PNG && mime == "image/png" =>
            {
                Some((*w, *h, *y))
            }
            _ => None,
        });
        let (iw, ih, iy) = image.expect("24pt mark");
        assert_eq!((iw, ih), (ew, eh), "paint must keep the 24pt on-page floor");
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
        assert!(
            list.ops.iter().any(|op| {
                matches!(
                    op,
                    Op::Text { text, color, .. }
                        if text.contains("dock") && *color == docagent_layout::BODY_FG
                )
            }),
            "task text must stay letter body #134e4a"
        );
        assert!(
            list.ops.iter().all(|op| {
                !matches!(op, Op::Text { color, text, .. } if text.contains("September") && *color == [0, 0, 0, 255])
            }),
            "body paint must not emit UA black"
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
