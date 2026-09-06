//! Markdown ↔ IR. Headings, paragraphs, and pipe tables.
//!
//! This is a document interchange codec, not a full CommonMark parser.

#![forbid(unsafe_code)]

use docagent_model::{
    Block, Document, NumberFormat, NumberingRef, Paragraph, Run, RunContent, Section, Table,
};

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
        let nest = list_nest(trimmed);
        let content = trimmed.trim_start();
        if let Some(rest) = content.strip_prefix("# ") {
            section.body.push(Block::Paragraph(heading(rest, 1, 1800, 200, 400)));
        } else if let Some(rest) = content.strip_prefix("## ") {
            section.body.push(Block::Paragraph(heading(rest, 2, 1400, 360, 240)));
        } else if let Some(rest) = content
            .strip_prefix("- ")
            .or_else(|| content.strip_prefix("* "))
        {
            section.body.push(Block::Paragraph(bullet_item(rest, nest)));
        } else if let Some((n, rest)) = ordered_item(content) {
            section.body.push(Block::Paragraph(decimal_item(rest, n, nest)));
        } else {
            let mut p = paragraph_from_inlines(trimmed);
            p.space_after = 200;
            section.body.push(Block::Paragraph(p));
        }
    }
    flush_table(&mut section, &mut table_rows);
    doc.sections.push(section);
    Ok(doc)
}

fn ordered_item(line: &str) -> Option<(u32, &str)> {
    let (num, rest) = line.split_once(". ")?;
    if num.is_empty() || !num.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u32 = num.parse().ok()?;
    (n > 0).then_some((n, rest))
}

fn list_nest(line: &str) -> u8 {
    let spaces = line.bytes().take_while(|b| *b == b' ').count();
    (spaces / 2).min(3) as u8
}

fn list_indent(nest: u8) -> i32 {
    1440i32.saturating_mul(i32::from(nest) + 1)
}

fn paragraph_from_inlines(text: &str) -> Paragraph {
    let mut p = Paragraph::from_text("");
    p.runs = parse_runs(text);
    if p.runs.is_empty() {
        p.runs.push(Run::text(""));
    }
    p
}

fn parse_runs(text: &str) -> Vec<Run> {
    let chars: Vec<char> = text.chars().collect();
    let mut runs = Vec::new();
    let mut buf = String::new();
    let mut bold = false;
    let mut italic = false;
    let mut i = 0;
    let flush = |runs: &mut Vec<Run>, buf: &mut String, bold: bool, italic: bool| {
        if buf.is_empty() {
            return;
        }
        let mut run = Run::text(std::mem::take(buf));
        run.style.bold = bold;
        run.style.italic = italic;
        runs.push(run);
    };
    while i < chars.len() {
        if chars[i] == '*' && i + 1 < chars.len() && chars[i + 1] == '*' {
            flush(&mut runs, &mut buf, bold, italic);
            bold = !bold;
            i += 2;
            continue;
        }
        if chars[i] == '*' || chars[i] == '_' {
            flush(&mut runs, &mut buf, bold, italic);
            italic = !italic;
            i += 1;
            continue;
        }
        buf.push(chars[i]);
        i += 1;
    }
    flush(&mut runs, &mut buf, bold, italic);
    runs
}

fn decimal_item(text: &str, n: u32, nest: u8) -> Paragraph {
    let mut p = paragraph_from_inlines(text);
    p.space_after = 80;
    p.indent_left = list_indent(nest);
    p.numbering = Some(NumberingRef {
        definition_id: 1,
        level: nest,
        start: Some(n),
        format: Some(NumberFormat::Decimal),
    });
    p
}

fn bullet_item(text: &str, nest: u8) -> Paragraph {
    let mut p = paragraph_from_inlines(text);
    p.space_after = 80;
    p.indent_left = list_indent(nest);
    p.numbering = Some(NumberingRef {
        definition_id: 0,
        level: nest,
        start: None,
        format: Some(NumberFormat::Bullet),
    });
    p
}

fn heading(text: &str, level: u8, size_hu: i32, before: i32, after: i32) -> Paragraph {
    let mut p = paragraph_from_inlines(text);
    p.outline_level = Some(level);
    p.space_before = before;
    p.space_after = after;
    for run in &mut p.runs {
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
    for (ri, row) in table.rows.iter_mut().enumerate() {
        let header = ri == 0;
        row.header = header;
        for cell in &mut row.cells {
            for b in &mut cell.blocks {
                if let Block::Paragraph(p) = b {
                    let text = p.plain_text();
                    *p = paragraph_from_inlines(&text);
                    if header {
                        for run in &mut p.runs {
                            run.style.bold = true;
                        }
                    }
                }
            }
        }
    }
    table.header_row_count = 1;
    let w = docagent_model::HU_PER_POINT;
    table.borders.top.width = w;
    table.borders.bottom.width = w;
    table.borders.left.width = w;
    table.borders.right.width = w;
    table.borders.inside_h.width = w;
    table.borders.inside_v.width = w;
    section.body.push(Block::Table(table));
}

pub fn write(doc: &Document) -> Result<Vec<u8>, Error> {
    let mut out = String::new();
    for section in &doc.sections {
        for block in &section.body {
            match block {
                Block::Paragraph(p) => {
                    match p.outline_level {
                        Some(1) => out.push_str(&format!("# {}\n\n", p.plain_text())),
                        Some(2) => out.push_str(&format!("## {}\n\n", p.plain_text())),
                        _ if p.numbering.as_ref().is_some_and(|n| {
                            n.format == Some(NumberFormat::Decimal)
                        }) =>
                        {
                            let nref = p.numbering.as_ref().unwrap();
                            let i = nref.start.unwrap_or(1);
                            let pad = "  ".repeat(nref.level as usize);
                            out.push_str(&format!("{pad}{i}. {}\n", write_runs(p)));
                        }
                        _ if p.numbering.is_some() => {
                            let pad = "  ".repeat(
                                p.numbering.as_ref().map(|n| n.level as usize).unwrap_or(0),
                            );
                            out.push_str(&format!("{pad}- {}\n", write_runs(p)));
                        }
                        _ => out.push_str(&format!("{}\n\n", write_runs(p))),
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
                                        Block::Paragraph(p) => Some(write_runs(p)),
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

fn write_runs(p: &Paragraph) -> String {
    let mut s = String::new();
    for run in &p.runs {
        let RunContent::Text(t) = &run.content else {
            continue;
        };
        if t.is_empty() {
            continue;
        }
        if run.style.bold && run.style.italic {
            s.push_str("***");
            s.push_str(t);
            s.push_str("***");
        } else if run.style.bold {
            s.push_str("**");
            s.push_str(t);
            s.push_str("**");
        } else if run.style.italic {
            s.push('_');
            s.push_str(t);
            s.push('_');
        } else {
            s.push_str(t);
        }
    }
    s
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
    fn table_cells_keep_emphasis() {
        let src = b"| Hop | File |\n| **GPU-free** | _byte-for-byte_ |\n";
        let doc = read(src).unwrap();
        let Block::Table(t) = &doc.sections[0].body[0] else {
            panic!("table");
        };
        let Block::Paragraph(h) = &t.rows[0].cells[0].blocks[0] else {
            panic!("header");
        };
        assert!(h.runs.iter().any(|r| r.style.bold && matches!(&r.content, RunContent::Text(t) if t.contains("Hop"))));
        let Block::Paragraph(p) = &t.rows[1].cells[0].blocks[0] else {
            panic!("bold cell");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.bold && matches!(&r.content, RunContent::Text(t) if t.contains("GPU-free"))
        }));
        let Block::Paragraph(p) = &t.rows[1].cells[1].blocks[0] else {
            panic!("italic cell");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.italic && matches!(&r.content, RunContent::Text(t) if t.contains("byte-for-byte"))
        }));
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(back.contains("**GPU-free**"), "{back}");
        assert!(back.contains("_byte-for-byte_"), "{back}");
    }

    #[test]
    fn bullets_keep_numbering_on_roundtrip() {
        let src = b"# Title\n\n- dock is clear\n- replay the convert\n";
        let doc = read(src).unwrap();
        let items: Vec<_> = doc.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) if p.numbering.is_some() => Some(p.plain_text()),
                _ => None,
            })
            .collect();
        assert_eq!(items, ["dock is clear", "replay the convert"]);
        let back = write(&doc).unwrap();
        let again = read(&back).unwrap();
        let n = again.sections[0]
            .body
            .iter()
            .filter(|b| matches!(b, Block::Paragraph(p) if p.numbering.is_some()))
            .count();
        assert_eq!(n, 2);
    }

    #[test]
    fn ordered_items_keep_start_on_roundtrip() {
        let src = b"1. Hash the input\n2. Run Convert\n";
        let doc = read(src).unwrap();
        let nums: Vec<_> = doc.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => p.numbering.as_ref().and_then(|n| n.start),
                _ => None,
            })
            .collect();
        assert_eq!(nums, [1, 2]);
        let back = write(&doc).unwrap();
        let text = String::from_utf8(back).unwrap();
        assert!(text.contains("1. Hash the input"));
        assert!(text.contains("2. Run Convert"));
    }

    #[test]
    fn nested_bullets_keep_level() {
        let src = b"- dock\n  - bay 4\n- hashes\n";
        let doc = read(src).unwrap();
        let levels: Vec<_> = doc.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => p.numbering.as_ref().map(|n| n.level),
                _ => None,
            })
            .collect();
        assert_eq!(levels, [0, 1, 0]);
        let back = write(&doc).unwrap();
        let text = String::from_utf8(back).unwrap();
        assert!(text.contains("  - bay 4"), "{text}");
    }

    #[test]
    fn emphasis_splits_runs_and_roundtrips() {
        let src = b"one **two** three _four_\n";
        let doc = read(src).unwrap();
        let Block::Paragraph(p) = &doc.sections[0].body[0] else {
            panic!("para");
        };
        let flags: Vec<_> = p
            .runs
            .iter()
            .map(|r| (r.style.bold, r.style.italic, match &r.content {
                RunContent::Text(t) => t.as_str(),
                _ => "",
            }))
            .collect();
        assert_eq!(
            flags,
            [
                (false, false, "one "),
                (true, false, "two"),
                (false, false, " three "),
                (false, true, "four"),
            ]
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(back.contains("**two**"), "{back}");
        assert!(back.contains("_four_"), "{back}");
        assert!(!back.contains("**one"), "{back}");
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
