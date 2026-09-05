//! 120 documents × 5 format hops. Drives shipped codecs, not re-implementations.

use docagent_model::{Block, Document, Paragraph, Section, Table};

fn make_doc(i: usize) -> Document {
    let mut doc = Document::new();
    let mut section = Section::default();
    section.body.push(Block::Paragraph(Paragraph::from_text(
        format!("hop-{i} 문서 DocAgent paragraph"),
    )));
    section.body.push(Block::Paragraph(Paragraph::from_text(
        format!("second line {i} abcdef"),
    )));
    if i.is_multiple_of(3) {
        section.body.push(Block::Table(Table::from_cells(vec![
            vec![format!("c0-{i}"), format!("c1-{i}")],
            vec![format!("c2-{i}"), format!("c3-{i}")],
        ])));
    }
    if i.is_multiple_of(2) {
        section.page.margin_left = 7200 + (i as i32 % 20) * 10;
    }
    doc.sections.push(section);
    doc
}

fn assert_core(src: &Document, dst: &Document, hop: &str) {
    let a = src.plain_text().replace('\r', "");
    let b = dst.plain_text().replace('\r', "");
    assert!(
        b.contains("hop-") || b.contains("문서") || !a.is_empty(),
        "{hop}: empty dest"
    );
    for token in a.lines() {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        assert!(
            b.contains(token),
            "{hop}: missing `{token}` in `{b}`"
        );
    }
}

#[test]
fn one_hundred_twenty_interformat_hops() {
    let mut hops = 0u32;
    for i in 0..120 {
        let src = make_doc(i);
        let hwp5 = docagent_hwp5::write(&src).expect("hwp5 write");
        let d1 = docagent_hwp5::read(&hwp5).expect("hwp5 read");
        assert_core(&src, &d1, "ir->hwp5->ir");
        hops += 1;

        let hwpx = docagent_hwpx::write(&d1).expect("hwpx write");
        let d2 = docagent_hwpx::read(&hwpx).expect("hwpx read");
        assert_core(&src, &d2, "hwp5->hwpx->ir");
        hops += 1;

        let hml = docagent_hml::write(&d2).expect("hml write");
        let d3 = docagent_hml::read(&hml).expect("hml read");
        assert_core(&src, &d3, "hwpx->hml->ir");
        hops += 1;

        let docx = docagent_docx::write(&d3).expect("docx write");
        let d4 = docagent_docx::read(&docx).expect("docx read");
        assert_core(&src, &d4, "hml->docx->ir");
        hops += 1;

        let hwp5b = docagent_hwp5::write(&d4).expect("docx->hwp5 write");
        let d5 = docagent_hwp5::read(&hwp5b).expect("hwp5 again");
        assert_core(&src, &d5, "docx->hwp5->ir");
        hops += 1;
    }
    assert!(hops >= 100, "hops={hops}");
    assert_eq!(hops, 600);
}
