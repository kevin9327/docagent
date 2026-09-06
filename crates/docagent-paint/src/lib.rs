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
}
