//! Semantic HTML from IR (not from the paint tree). Search/RAG/a11y.

#![forbid(unsafe_code)]

use docagent_model::{Block, Document, Paragraph, RunContent, Table};

pub fn to_html(doc: &Document) -> String {
    let mut s = String::from(
        r#"<!DOCTYPE html><html lang="en"><head><meta charset="utf-8"/><title>DocAgent</title><style>body{margin:0;background:#f8fafc;color:#111827}article{max-width:48rem;margin:2rem auto;padding:2.5rem 2.75rem;background:#fff;border:1px solid #e2e8f0;font-family:"Segoe UI",Roboto,Helvetica,Arial,sans-serif;line-height:1.45}h1{font-size:1.75rem;margin:0 0 .6rem;letter-spacing:-.02em}h2{font-size:1.15rem;margin:1.1rem 0 .45rem;color:#134e4a}p{margin:0 0 .7rem}strong{font-weight:700}em{font-style:italic}ul,ol{margin:0 0 1rem;padding-left:1.25rem}ul ul,ol ul,ul ol,ol ol{margin:.25rem 0}li{margin:0 0 .35rem}table{border-collapse:collapse;margin:1rem 0;width:100%}th,td{border:1px solid #cbd5e1;padding:.4rem .65rem;text-align:left;font-size:.95rem}th{background:#f0fdfa;color:#134e4a}</style></head><body>"#,
    );
    for (i, section) in doc.sections.iter().enumerate() {
        s.push_str(&format!(r#"<article data-section="{i}">"#));
        if let Some(h) = &section.header {
            s.push_str("<header>");
            push_blocks(&mut s, &h.blocks);
            s.push_str("</header>");
        }
        s.push_str("<section>");
        push_blocks(&mut s, &section.body);
        s.push_str("</section>");
        if let Some(f) = &section.footer {
            s.push_str("<footer>");
            push_blocks(&mut s, &f.blocks);
            s.push_str("</footer>");
        }
        s.push_str("</article>");
    }
    s.push_str("</body></html>");
    s
}

fn list_format(block: &Block) -> Option<docagent_model::NumberFormat> {
    match block {
        Block::Paragraph(p) => p.numbering.as_ref().and_then(|n| n.format),
        _ => None,
    }
}

fn list_level(block: &Block) -> Option<u8> {
    match block {
        Block::Paragraph(p) => p.numbering.as_ref().map(|n| n.level),
        _ => None,
    }
}

fn emit_list(s: &mut String, blocks: &[Block], i: &mut usize) {
    let Some(fmt) = list_format(&blocks[*i]) else {
        return;
    };
    let base = list_level(&blocks[*i]).unwrap_or(0);
    match fmt {
        docagent_model::NumberFormat::Bullet => s.push_str("<ul>"),
        docagent_model::NumberFormat::Decimal => {
            let start = match &blocks[*i] {
                Block::Paragraph(p) => p.numbering.as_ref().and_then(|n| n.start).unwrap_or(1),
                _ => 1,
            };
            if start == 1 {
                s.push_str("<ol>");
            } else {
                s.push_str(&format!("<ol start=\"{start}\">"));
            }
        }
        _ => return,
    }
    while *i < blocks.len() {
        let Some(f) = list_format(&blocks[*i]) else {
            break;
        };
        let lvl = list_level(&blocks[*i]).unwrap_or(0);
        if lvl < base {
            break;
        }
        if lvl > base {
            emit_list(s, blocks, i);
            continue;
        }
        if f != fmt {
            break;
        }
        let Block::Paragraph(p) = &blocks[*i] else {
            break;
        };
        s.push_str("<li>");
        push_runs(s, p);
        *i += 1;
        if *i < blocks.len() && list_level(&blocks[*i]).is_some_and(|nl| nl > base) {
            emit_list(s, blocks, i);
        }
        s.push_str("</li>");
    }
    match fmt {
        docagent_model::NumberFormat::Bullet => s.push_str("</ul>"),
        docagent_model::NumberFormat::Decimal => s.push_str("</ol>"),
        _ => {}
    }
}

fn push_blocks(s: &mut String, blocks: &[Block]) {
    let mut i = 0;
    while i < blocks.len() {
        if list_format(&blocks[i]).is_some() {
            emit_list(s, blocks, &mut i);
        } else {
            push_block(s, &blocks[i]);
            i += 1;
        }
    }
}

fn push_block(s: &mut String, block: &Block) {
    match block {
        Block::Paragraph(p) => push_para(s, p),
        Block::Table(t) => push_table(s, t),
        Block::Float(_) => s.push_str("<aside></aside>"),
        Block::Break(_) => s.push_str("<hr/>"),
    }
}

fn push_para(s: &mut String, p: &Paragraph) {
    let tag = match p.outline_level {
        Some(1) => "h1",
        Some(2) => "h2",
        Some(3) => "h3",
        _ => "p",
    };
    s.push('<');
    s.push_str(tag);
    s.push('>');
    push_runs(s, p);
    s.push_str("</");
    s.push_str(tag);
    s.push('>');
}

fn push_runs(s: &mut String, p: &Paragraph) {
    for run in &p.runs {
        let RunContent::Text(t) = &run.content else {
            continue;
        };
        if t.is_empty() {
            continue;
        }
        let escaped = escape(t);
        if run.style.bold {
            s.push_str("<strong>");
        }
        if run.style.italic {
            s.push_str("<em>");
        }
        s.push_str(&escaped);
        if run.style.italic {
            s.push_str("</em>");
        }
        if run.style.bold {
            s.push_str("</strong>");
        }
    }
}

fn push_table(s: &mut String, t: &Table) {
    s.push_str("<table>");
    for (ri, row) in t.rows.iter().enumerate() {
        s.push_str("<tr>");
        for cell in &row.cells {
            let tag = if row.header || ri < t.header_row_count as usize {
                "th"
            } else {
                "td"
            };
            s.push_str(&format!("<{tag}>"));
            for b in &cell.blocks {
                if let Block::Paragraph(p) = b {
                    s.push_str(&escape(&p.plain_text()));
                }
            }
            s.push_str(&format!("</{tag}>"));
        }
        s.push_str("</tr>");
    }
    s.push_str("</table>");
}

fn escape(t: &str) -> String {
    t.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use docagent_model::{Paragraph, Section};

    #[test]
    fn html_has_semantic_structure() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("hello")));
        section
            .body
            .push(Block::Table(Table::from_cells(vec![vec!["a".into(), "b".into()]])));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains("<article"));
        assert!(html.contains("<section>"));
        assert!(html.contains("<p>hello</p>"));
        assert!(html.contains("<table>"));
        assert!(html.contains("<td>a</td>"));
    }

    #[test]
    fn headings_are_semantic() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h = Paragraph::from_text("Title");
        h.outline_level = Some(1);
        section.body.push(Block::Paragraph(h));
        let mut h2 = Paragraph::from_text("Section");
        h2.outline_level = Some(2);
        section.body.push(Block::Paragraph(h2));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains("<h1>") && html.contains("Title") && html.contains("</h1>"));
        assert!(html.contains("<h2>") && html.contains("Section") && html.contains("</h2>"));
    }

    #[test]
    fn bullets_are_lists() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut a = Paragraph::from_text("dock");
        a.numbering = Some(docagent_model::NumberingRef {
            definition_id: 0,
            level: 0,
            start: None,
            format: Some(docagent_model::NumberFormat::Bullet),
        });
        let mut b = Paragraph::from_text("replay");
        b.numbering = Some(docagent_model::NumberingRef {
            definition_id: 0,
            level: 0,
            start: None,
            format: Some(docagent_model::NumberFormat::Bullet),
        });
        section.body.push(Block::Paragraph(a));
        section.body.push(Block::Paragraph(b));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains("<ul>"));
        assert!(html.contains("<li>dock</li>"));
        assert!(html.contains("<li>replay</li>"));
        assert!(!html.contains("<p>dock</p>"));
    }

    #[test]
    fn ordered_items_are_ol() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut a = Paragraph::from_text("Hash the input");
        a.numbering = Some(docagent_model::NumberingRef {
            definition_id: 1,
            level: 0,
            start: Some(1),
            format: Some(docagent_model::NumberFormat::Decimal),
        });
        let mut b = Paragraph::from_text("Run Convert");
        b.numbering = Some(docagent_model::NumberingRef {
            definition_id: 1,
            level: 0,
            start: Some(2),
            format: Some(docagent_model::NumberFormat::Decimal),
        });
        section.body.push(Block::Paragraph(a));
        section.body.push(Block::Paragraph(b));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains("<ol>"));
        assert!(html.contains("<li>Hash the input</li>"));
        assert!(html.contains("<li>Run Convert</li>"));
        assert!(!html.contains("<ul>"));
    }

    #[test]
    fn nested_bullets_are_nested_ul() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut a = Paragraph::from_text("dock");
        a.numbering = Some(docagent_model::NumberingRef {
            definition_id: 0,
            level: 0,
            start: None,
            format: Some(docagent_model::NumberFormat::Bullet),
        });
        let mut b = Paragraph::from_text("bay 4");
        b.numbering = Some(docagent_model::NumberingRef {
            definition_id: 0,
            level: 1,
            start: None,
            format: Some(docagent_model::NumberFormat::Bullet),
        });
        section.body.push(Block::Paragraph(a));
        section.body.push(Block::Paragraph(b));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains("<ul><li>dock<ul><li>bay 4</li></ul></li></ul>"), "{html}");
    }

    #[test]
    fn emphasis_emits_strong_and_em() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("two");
        p.runs[0].style.bold = true;
        let mut q = Paragraph::from_text("four");
        q.runs[0].style.italic = true;
        section.body.push(Block::Paragraph(p));
        section.body.push(Block::Paragraph(q));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains("<strong>two</strong>"), "{html}");
        assert!(html.contains("<em>four</em>"), "{html}");
    }
}
