//! Semantic HTML from IR (not from the paint tree). Search/RAG/a11y.

#![forbid(unsafe_code)]

use docagent_model::{Block, Document, Paragraph, Table};

pub fn to_html(doc: &Document) -> String {
    let mut s = String::from(
        r#"<!DOCTYPE html><html lang="en"><head><meta charset="utf-8"/><title>DocAgent</title><style>body{margin:0;background:#f8fafc;color:#111827}article{max-width:48rem;margin:2rem auto;padding:2.5rem 2.75rem;background:#fff;border:1px solid #e2e8f0;font-family:"Segoe UI",Roboto,Helvetica,Arial,sans-serif;line-height:1.45}h1{font-size:1.75rem;margin:0 0 .6rem;letter-spacing:-.02em}h2{font-size:1.15rem;margin:1.1rem 0 .45rem;color:#134e4a}p{margin:0 0 .7rem}table{border-collapse:collapse;margin:1rem 0;width:100%}th,td{border:1px solid #cbd5e1;padding:.4rem .65rem;text-align:left;font-size:.95rem}th{background:#f0fdfa;color:#134e4a}</style></head><body>"#,
    );
    for (i, section) in doc.sections.iter().enumerate() {
        s.push_str(&format!(r#"<article data-section="{i}">"#));
        if let Some(h) = &section.header {
            s.push_str("<header>");
            for b in &h.blocks {
                push_block(&mut s, b);
            }
            s.push_str("</header>");
        }
        s.push_str("<section>");
        for b in &section.body {
            push_block(&mut s, b);
        }
        s.push_str("</section>");
        if let Some(f) = &section.footer {
            s.push_str("<footer>");
            for b in &f.blocks {
                push_block(&mut s, b);
            }
            s.push_str("</footer>");
        }
        s.push_str("</article>");
    }
    s.push_str("</body></html>");
    s
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
    s.push_str("<");
    s.push_str(tag);
    s.push_str(">");
    s.push_str(&escape(&p.plain_text()));
    s.push_str("</");
    s.push_str(tag);
    s.push_str(">");
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
        assert!(html.contains("<h1>Title</h1>"));
        assert!(html.contains("<h2>Section</h2>"));
    }
}
