//! Single paint backend IR: a flat display list, rkyv-serializable.

#![forbid(unsafe_code)]

use dochwp_layout::FragmentTree;
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
        for rect in &page.rects {
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
                color: [0, 0, 0, 255],
            });
        }
    }
    DisplayList { ops }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dochwp_font::FontSet;
    use dochwp_layout::layout_document;
    use dochwp_model::{Block, Document, Paragraph, Section};

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
}
