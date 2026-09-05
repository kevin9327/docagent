//! Markdown ↔ IR. Headings, paragraphs, and pipe tables.
//!
//! This is a document interchange codec, not a full CommonMark parser.

#![forbid(unsafe_code)]

use docagent_model::{Block, Document, Paragraph, Section, Table};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not markdown text")]
    NotMarkdown,
}

pub fn sniff(bytes: &[u8]) -> bool {
    if bytes.is_empty() || bytes[0] == 0 {
        return false;
    }
    if bytes.len() >= 2 && bytes[0..2] == [0x50, 0x4B] {
        return false;
    }
    if bytes.len() >= 8 && bytes[0..8] == [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1] {
        return false;
    }
    if bytes.starts_with(b"HWP Document File") || bytes.starts_with(b"<?xml") || bytes.starts_with(b"{")
    {
        return false;
    }
    std::str::from_utf8(bytes).is_ok()
}

pub fn read(bytes: &[u8]) -> Result<Document, Error> {
    let text = std::str::from_utf8(bytes).map_err(|_| Error::NotMarkdown)?;
    let mut doc = Document::new();
    let mut section = Section::default();
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_end();
        if trimmed.starts_with('|') && trimmed.contains('|') {
            let cells: Vec<String> = trimmed
                .trim_matches('|')
                .split('|')
                .map(|c| c.trim().to_string())
                .filter(|c| !c.chars().all(|ch| ch == '-'))
                .collect();
            if !cells.is_empty() {
                table_rows.push(cells);
            }
            continue;
        }
        flush_table(&mut section, &mut table_rows);
        if trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("# ") {
            section.body.push(Block::Paragraph(heading(rest, 1, 1800, 200, 400)));
        } else if let Some(rest) = trimmed.strip_prefix("## ") {
            section.body.push(Block::Paragraph(heading(rest, 2, 1400, 360, 240)));
        } else {
            let item = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
                .unwrap_or(trimmed);
            let mut p = Paragraph::from_text(item);
            p.space_after = 200;
            section.body.push(Block::Paragraph(p));
        }
    }
    flush_table(&mut section, &mut table_rows);
    doc.sections.push(section);
    Ok(doc)
}

fn heading(text: &str, level: u8, size_hu: i32, before: i32, after: i32) -> Paragraph {
    let mut p = Paragraph::from_text(text);
    p.outline_level = Some(level);
    p.space_before = before;
    p.space_after = after;
    if let Some(run) = p.runs.first_mut() {
        run.style.size = size_hu;
        run.style.bold = true;
    }
    p
}

fn flush_table(section: &mut Section, rows: &mut Vec<Vec<String>>) {
    if rows.is_empty() {
        return;
    }
    let mut table = Table::from_cells(std::mem::take(rows));
    if let Some(first) = table.rows.first_mut() {
        first.header = true;
    }
    table.header_row_count = 1;
    section.body.push(Block::Table(table));
}

pub fn write(doc: &Document) -> Result<Vec<u8>, Error> {
    let mut out = String::new();
    for section in &doc.sections {
        for block in &section.body {
            match block {
                Block::Paragraph(p) => {
                    let t = p.plain_text();
                    match p.outline_level {
                        Some(1) => out.push_str(&format!("# {t}\n\n")),
                        Some(2) => out.push_str(&format!("## {t}\n\n")),
                        _ => out.push_str(&format!("{t}\n\n")),
                    }
                }
                Block::Table(table) => {
                    for row in &table.rows {
                        let cells: Vec<String> = row
                            .cells
                            .iter()
                            .map(|c| {
                                c.blocks
                                    .iter()
                                    .filter_map(|b| match b {
                                        Block::Paragraph(p) => Some(p.plain_text()),
                                        _ => None,
                                    })
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            })
                            .collect();
                        out.push('|');
                        out.push(' ');
                        out.push_str(&cells.join(" | "));
                        out.push_str(" |\n");
                    }
                    out.push('\n');
                }
                Block::Float(_) | Block::Break(_) => {}
            }
        }
    }
    Ok(out.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_heading_and_table() {
        let src = b"# Title\n\nHello world\n\n| a | b |\n| c | d |\n";
        let doc = read(src).unwrap();
        assert!(doc.plain_text().contains("Title"));
        assert!(doc.plain_text().contains("Hello world"));
        assert_eq!(doc.table_count(), 1);
        let back = write(&doc).unwrap();
        let again = read(&back).unwrap();
        assert!(again.plain_text().contains("Title"));
        assert!(again.plain_text().contains("a"));
    }

    #[test]
    fn headings_carry_size_for_layout() {
        let doc = read(b"# Title\n\n## Sub\n\nBody\n").unwrap();
        let h1 = match &doc.sections[0].body[0] {
            Block::Paragraph(p) => p,
            _ => panic!("h1"),
        };
        assert_eq!(h1.outline_level, Some(1));
        assert_eq!(h1.runs[0].style.size, 1800);
        assert!(h1.runs[0].style.bold);
    }

    #[test]
    fn sniff_rejects_ole_and_zip() {
        assert!(!sniff(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]));
        assert!(!sniff(b"PK\x03\x04rest"));
        assert!(sniff(b"# Hello\n"));
    }
}
