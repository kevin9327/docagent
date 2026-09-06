//! `examples/letter.md` → DOCX must look like Word: heading, table, and a
//! visible Harbor `a:blip` PNG (24pt floor, not a 16px/12pt speck).

use std::io::{Cursor, Read};
use std::path::Path;

use docagent_model::{Block, InlineObject, NumberFormat, RunContent};
use zip::ZipArchive;

const PNG_MAGIC: &[u8] = &[0x89, b'P', b'N', b'G'];
/// 24pt in EMU: 2400 HWPUNIT × 127.
const MIN_VISIBLE_EMU: i64 = 304_800;

#[test]
fn letter_md_docx_includes_harbor_mark_as_blip() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("!["),
        "examples/letter.md must contain a markdown image"
    );
    assert!(
        text.contains("mark.png"),
        "examples/letter.md must reference mark.png"
    );
    assert!(
        text.contains("\n---\n"),
        "examples/letter.md must keep the thematic break"
    );
    assert!(
        text.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
        "examples/letter.md must keep the DocAgent hyperlink"
    );
    assert!(
        text.contains("==three hashes=="),
        "examples/letter.md must keep the ==mark== highlight"
    );
    assert!(
        text.contains("`prove`"),
        "examples/letter.md must keep the inline `prove` code span"
    );
    assert!(
        text.contains("~~guess~~"),
        "examples/letter.md must keep the strikethrough Word paints as w:strike"
    );
    assert!(
        text.contains("__09:00 local__"),
        "examples/letter.md must keep the underline Word paints as w:u"
    );
    assert!(
        text.contains("PDF/^A^"),
        "examples/letter.md must keep the PDF/A superscript Word paints as w:vertAlign"
    );
    assert!(
        text.contains("H~2~O"),
        "examples/letter.md must keep the H2O subscript Word paints as w:vertAlign"
    );
    assert!(
        text.contains("- [x] Receiving dock is clear"),
        "examples/letter.md must keep the checked task item"
    );
    assert!(
        text.contains("- [ ] Replay the convert instead of hoping"),
        "examples/letter.md must keep the unchecked task item"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    let docx = docagent_docx::write(&doc).expect("docx write");
    assert!(docagent_docx::sniff(&docx), "written bytes must sniff as DOCX");

    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    {
        let mut file = zip
            .by_name("word/document.xml")
            .expect("word/document.xml");
        file.read_to_string(&mut document_xml).unwrap();
    }
    assert!(
        document_xml.contains(r#"w:val="Heading1""#)
            || document_xml.contains(r#"w:val="Heading2""#),
        "DOCX from letter.md must use a Word heading style: {document_xml}"
    );
    assert!(
        document_xml.contains("<w:tbl>"),
        "DOCX from letter.md must include a table: {document_xml}"
    );
    assert!(
        document_xml.contains("<w:tblBorders>"),
        "table must have visible Word borders, not an empty tblPr: {document_xml}"
    );
    let grid = tbl_borders_block(&document_xml).expect("w:tblBorders on the letter table");
    assert!(
        grid.contains(r#"w:color="CBD5E1""#)
            && grid.contains(r#"w:sz="6""#)
            && grid.contains("<w:insideH")
            && grid.contains("<w:insideV")
            && !grid.contains(r#"w:color="000000""#),
        "letter hop grid must be 1px #cbd5e1, not Word TableGrid black: {grid}"
    );
    assert!(
        document_xml.contains("Hop") && document_xml.contains("letter.md"),
        "table must keep the letter hop rows: {document_xml}"
    );
    let pad_x = mar_side_twips(&document_xml, "w:tblCellMar", "left")
        .expect("w:tblCellMar left on the letter table");
    let pad_y = mar_side_twips(&document_xml, "w:tblCellMar", "top")
        .expect("w:tblCellMar top on the letter table");
    assert!(
        pad_x >= 156 && pad_y >= 96,
        "table cell padding must be letter CSS 0.65rem/0.4rem (156/96 twips), got {pad_x}×{pad_y}: {document_xml}"
    );
    let tc_x = mar_side_twips(&document_xml, "w:tcMar", "left").expect("w:tcMar left");
    let tc_y = mar_side_twips(&document_xml, "w:tcMar", "top").expect("w:tcMar top");
    assert!(
        tc_x >= 156 && tc_y >= 96,
        "w:tcMar must keep cell padding, got {tc_x}×{tc_y}: {document_xml}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "DOCX from letter.md must include the Harbor picture as a:blip in word/document.xml: {document_xml}"
    );
    assert!(
        document_xml.contains(
            r#"<pic:pic xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture">"#
        ),
        "python-docx puts xmlns:pic on pic:pic so Word paints a:blip: {document_xml}"
    );
    assert!(
        document_xml.contains("Harbor mark"),
        "Harbor mark alt must appear on wp:docPr: {document_xml}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must be at least 24pt ({MIN_VISIBLE_EMU} EMU), got {cx}×{cy}: {document_xml}"
    );
    let mark_p = harbor_paragraph(&document_xml).expect("paragraph wrapping the Harbor a:blip");
    assert_eq!(
        spacing_before_twips(mark_p),
        Some(144),
        "gap before Harbor mark must be letter img 0.6rem (144 twips), not Word 0 or H2 4pt: {mark_p}"
    );
    assert_eq!(
        spacing_after_twips(mark_p),
        Some(168),
        "gap after Harbor mark must be letter 0.7rem (168 twips), not Word 8pt or last-list 1rem: {mark_p}"
    );
    assert!(
        !mark_p.contains(r#"w:before="0""#)
            && !mark_p.contains(r#"w:before="16""#)
            && !mark_p.contains(r#"w:before="80""#),
        "Word 0 / IR 80 / H2 4pt before the mark is jammed vs WeasyPrint 0.6rem: {mark_p}"
    );
    assert!(
        !mark_p.contains(r#"w:after="160""#) && !mark_p.contains(r#"w:after="240""#),
        "Word Normal 8pt / last-list 1rem after the mark is jammed or sparse vs WeasyPrint 0.7rem: {mark_p}"
    );

    let mut styles_xml = String::new();
    {
        let mut file = zip.by_name("word/styles.xml").expect("word/styles.xml");
        file.read_to_string(&mut styles_xml).unwrap();
    }
    let h1_style = style_block(&styles_xml, "Heading1").expect("Heading1 style");
    let h2_style = style_block(&styles_xml, "Heading2").expect("Heading2 style");
    let normal_style = style_block(&styles_xml, "Normal").expect("Normal style");
    let h1_sz = style_sz(h1_style).expect("Heading1 w:sz");
    let h2_sz = style_sz(h2_style).expect("Heading2 w:sz");
    assert!(
        h1_sz >= 32,
        "Heading1 must be Word 16pt (sz 32), got {h1_sz}: {h1_style}"
    );
    assert!(
        h2_sz >= 26,
        "Heading2 must be Word 13pt (sz 26), got {h2_sz}: {h2_style}"
    );
    assert_eq!(
        h1_sz, 32,
        "Heading1 style must stay Word 16pt (python-docx/Pandoc), got {h1_sz}: {h1_style}"
    );
    assert_eq!(
        h2_sz, 26,
        "Heading2 style must stay Word 13pt (python-docx/Pandoc), got {h2_sz}: {h2_style}"
    );
    let h1_style_before = spacing_before_twips(h1_style).unwrap_or(0);
    let h2_style_before = spacing_before_twips(h2_style).unwrap_or(0);
    assert!(
        h1_style_before >= 240,
        "Heading1 style before must be Word 12pt (240 twips), got {h1_style_before}: {h1_style}"
    );
    assert!(
        h2_style_before >= 200,
        "Heading2 style before must be Word 10pt (200 twips), got {h2_style_before}: {h2_style}"
    );
    let normal_after = spacing_after_twips(normal_style).unwrap_or(0);
    assert_eq!(
        normal_after, 168,
        "Normal after must stay letter.css p 0.7rem (168 twips), not Word 8pt, got {normal_after}: {normal_style}"
    );
    let normal_line = spacing_attr(normal_style, "w:line").unwrap_or(0);
    assert!(
        normal_line >= 276 && normal_style.contains(r#"w:lineRule="auto""#),
        "Normal line must stay Word 1.15 (276), got {normal_line}: {normal_style}"
    );
    assert!(
        normal_style.contains(r#"<w:color w:val="134E4A"/>"#)
            && !normal_style.contains(r#"w:val="000000""#)
            && !normal_style.contains(r#"w:color="auto""#),
        "Normal type must stay letter #134e4a, not UA black: {normal_style}"
    );

    let h1_p = paragraph_containing(&document_xml, r#"w:val="Heading1""#)
        .expect("Heading1 paragraph");
    let h2_p = paragraph_containing(&document_xml, r#"w:val="Heading2""#)
        .expect("Heading2 paragraph");
    let body_p = paragraph_containing(&document_xml, "6 September 2026")
        .expect("body date paragraph");
    let hop_p = paragraph_containing(&document_xml, "Hop").expect("Hop cell paragraph");
    let h1_before = spacing_before_twips(h1_p).unwrap_or(0);
    let h1_after = spacing_after_twips(h1_p).unwrap_or(0);
    let h2_before = spacing_before_twips(h2_p).unwrap_or(0);
    let body_after = spacing_after_twips(body_p).unwrap_or(0);
    assert!(
        h1_before >= 240,
        "letter H1 before must be Word 12pt, got {h1_before}: {h1_p}"
    );
    assert!(
        h1_after >= 160,
        "letter H1 after must keep 8pt so H1+H2 is not a stacked outline, got {h1_after}: {h1_p}"
    );
    assert!(
        h2_before >= 200,
        "letter H2 before must be Word 10pt, got {h2_before}: {h2_p}"
    );
    assert!(
        !h1_p.contains("<w:sz "),
        "letter H1 run must not override Word 16pt with markdown CSS 21pt: {h1_p}"
    );
    assert!(
        !h2_p.contains("<w:sz "),
        "letter H2 run must not override Word 13pt with markdown CSS 13.5pt: {h2_p}"
    );
    let h1_run = run_containing(h1_p, ">Northwind Freight</w:t>").expect("H1 run");
    let h2_run = run_containing(h2_p, ">Delivery confirmation").expect("H2 run");
    assert!(
        h1_run.contains(r#"<w:color w:val="134E4A"/>"#)
            && !h1_run.contains(r#"w:val="2F5496""#)
            && !h1_run.contains(r#"w:val="000000""#),
        "letter H1 run must stay letter #134e4a, not Word Calibri blue: {h1_run}"
    );
    assert!(
        h2_run.contains(r#"<w:color w:val="134E4A"/>"#) && !h2_run.contains(r#"w:val="2F5496""#),
        "letter H2 run must stay letter #134e4a, not Word Calibri blue: {h2_run}"
    );
    assert!(
        h1_style.contains(r#"<w:color w:val="134E4A"/>"#)
            && h2_style.contains(r#"<w:color w:val="134E4A"/>"#)
            && !h1_style.contains(r#"w:val="2F5496""#)
            && !h2_style.contains(r#"w:val="2F5496""#),
        "Heading1/2 style type must stay letter #134e4a, not Word 2F5496: {h1_style} {h2_style}"
    );
    assert!(
        h1_p.contains("<w:pBdr>")
            && h1_p.contains("<w:bottom ")
            && h1_p.contains(r#"w:color="134E4A""#)
            && p_bdr_bottom_sz(h1_p).unwrap_or(0) >= 18,
        "letter H1 must keep letter.html 3px #134e4a bottom rule: {h1_p}"
    );
    assert!(
        h2_p.contains("<w:pBdr>")
            && h2_p.contains("<w:bottom ")
            && h2_p.contains(r#"w:color="0D9488""#)
            && p_bdr_bottom_sz(h2_p).unwrap_or(0) >= 12,
        "letter H2 must keep letter.html 2px #0d9488 bottom rule: {h2_p}"
    );
    assert!(
        h1_style.contains("<w:pBdr>")
            && h1_style.contains(r#"w:sz="18""#)
            && h1_style.contains(r#"w:color="134E4A""#),
        "Heading1 style must keep letter 3px #134e4a rule: {h1_style}"
    );
    assert!(
        h2_style.contains("<w:pBdr>")
            && h2_style.contains(r#"w:sz="12""#)
            && h2_style.contains(r#"w:color="0D9488""#),
        "Heading2 style must keep letter 2px #0d9488 rule: {h2_style}"
    );
    assert_eq!(
        body_after, 168,
        "letter body after must be letter.css p 0.7rem (168 twips), not Word 8pt, got {body_after}: {body_p}"
    );
    let body_line = spacing_attr(body_p, "w:line").unwrap_or(0);
    assert!(
        body_line >= 276,
        "letter body line must be Word 1.15 (276), got {body_line}: {body_p}"
    );
    let body_run = run_containing(body_p, ">6 September 2026</w:t>")
        .expect("body date run");
    assert!(
        body_run.contains(r#"<w:color w:val="134E4A"/>"#)
            && !body_run.contains(r#"w:val="000000""#)
            && !body_run.contains(r#"w:color="auto""#),
        "letter body run must stay #134e4a like quote/th, not UA black: {body_run}"
    );
    let hop_after = spacing_after_twips(hop_p).unwrap_or(0);
    assert!(
        hop_after < 160,
        "table cells must not inherit body 8pt after, got {hop_after}: {hop_p}"
    );
    assert!(
        hop_p.contains("<w:b/>"),
        "letter table header must be bold like Word/Pandoc: {hop_p}"
    );
    assert!(
        hop_p.contains(r#"w:val="134E4A""#),
        "letter table header text must stay letter teal #134e4a: {hop_p}"
    );
    let hop_run = run_containing(hop_p, ">Hop</w:t>").expect("Hop run");
    assert_eq!(
        style_sz(hop_run).expect("Hop w:sz"),
        23,
        "letter th must stay 0.95rem (sz 23), not body 11pt: {hop_run}"
    );
    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    let file_tc = cell_containing(&document_xml, "File").expect("File header cell");
    let proof_tc = cell_containing(&document_xml, "Proof").expect("Proof header cell");
    let input_tc = cell_containing(&document_xml, "Input").expect("Input body cell");
    let sha_tc = cell_containing(&document_xml, "input SHA-256").expect("SHA body cell");
    for (name, tc) in [("Hop", hop_tc), ("File", file_tc), ("Proof", proof_tc)] {
        assert!(
            tc.contains(r#"w:fill="CCFBF1""#),
            "letter {name} th must keep WeasyPrint teal fill #ccfbf1, not a sparse grid: {tc}"
        );
        assert!(
            tc.contains("<w:tcBorders>") && tc.contains(r#"<w:bottom "#),
            "letter {name} th must keep a bottom rule: {tc}"
        );
        let rule_sz = tc_bottom_sz(tc).unwrap_or(0);
        assert!(
            rule_sz >= 12 && tc.contains(r#"w:color="0D9488""#),
            "letter {name} th rule must be 2px #0d9488 (sz 12), got {rule_sz}: {tc}"
        );
        assert!(
            !tc.contains(r#"w:fill="FFFFFF""#)
                && !tc.contains(r#"w:fill="auto""#)
                && !tc.contains(r#"w:val="nil""#),
            "UA/reset {name} is a sparse grid, not WeasyPrint letter input: {tc}"
        );
    }
    assert!(
        !input_tc.contains(r#"w:fill="CCFBF1""#) && !sha_tc.contains(r#"w:fill="CCFBF1""#),
        "letter body td must not keep th fill: {input_tc} {sha_tc}"
    );
    for (name, tc) in [
        ("Hop", hop_tc),
        ("File", file_tc),
        ("Proof", proof_tc),
        ("Input", input_tc),
        ("SHA", sha_tc),
    ] {
        assert!(
            tc.contains(r#"<w:vAlign w:val="top"/>"#)
                && !tc.contains(r#"w:val="center""#)
                && !tc.contains(r#"w:val="bottom""#),
            "letter {name} cell must stay vertical-align top, not UA middle: {tc}"
        );
    }
    let table_grid = style_block(&styles_xml, "TableGrid").expect("TableGrid style");
    assert!(
        table_grid.contains(r#"w:type="firstRow""#)
            && table_grid.contains(r#"w:fill="CCFBF1""#)
            && table_grid.contains(r#"w:color="0D9488""#)
            && table_grid.contains(r#"w:sz="12""#),
        "TableGrid firstRow must keep letter th fill + 2px teal rule: {table_grid}"
    );
    assert!(
        table_grid.contains(r#"w:color="CBD5E1""#)
            && table_grid.contains(r#"w:sz="6""#)
            && !table_grid.contains(r#"w:color="000000""#),
        "TableGrid must keep letter 1px #cbd5e1, not Word black hairline: {table_grid}"
    );
    assert!(
        table_grid.contains(r#"<w:sz w:val="23"/>"#)
            && table_grid.contains(r#"<w:szCs w:val="23"/>"#),
        "TableGrid must keep letter 0.95rem (sz 23) so style-only cells are not body 11pt: {table_grid}"
    );
    assert!(
        table_grid.contains(r#"<w:vAlign w:val="top"/>"#)
            && !table_grid.contains(r#"w:val="center""#),
        "TableGrid must keep letter vertical-align top so style-only cells are not middle: {table_grid}"
    );

    let quote_p = paragraph_containing(&document_xml, "Replay is evidence")
        .expect("blockquote paragraph");
    let code_p = paragraph_containing(&document_xml, "docagent prove")
        .expect("code_block paragraph");
    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("list item paragraph");
    let bay_p = paragraph_containing(&document_xml, "Bay 4")
        .expect("nested list item paragraph");
    let last_task_p = paragraph_containing(&document_xml, "Replay the convert instead of hoping")
        .expect("last task list item");
    let last_num_p = paragraph_containing(&document_xml, "Compare the capsule")
        .expect("last decimal list item");
    let quote_before = spacing_before_twips(quote_p).unwrap_or(0);
    let quote_after = spacing_after_twips(quote_p).unwrap_or(0);
    let code_before = spacing_before_twips(code_p).unwrap_or(0);
    let code_after = spacing_after_twips(code_p).unwrap_or(0);
    assert!(
        (120..=240).contains(&quote_before),
        "letter quote before must be 6–12pt, not jammed or sparse, got {quote_before}: {quote_p}"
    );
    assert_eq!(
        quote_after, 240,
        "letter quote after must be letter.css 1rem (240 twips), not Word 8pt: {quote_p}"
    );
    assert!(
        !quote_p.contains(r#"w:after="160""#) && !quote_p.contains(r#"w:after="80""#),
        "Word Normal 8pt / jammed IR after the quote vs WeasyPrint 1rem: {quote_p}"
    );
    assert!(
        ind_left_twips(quote_p).unwrap_or(0) >= 720,
        "letter quote indent must be Word 0.5in, got {:?}: {quote_p}",
        ind_left_twips(quote_p)
    );
    assert!(
        quote_p.contains(r#"w:val="Quote""#)
            && quote_p.contains("<w:pBdr>")
            && quote_p.contains("<w:left ")
            && quote_p.contains(r#"w:color="134E4A""#),
        "letter blockquote must emit Word left pBdr matching letter.html 4px #134e4a: {quote_p}"
    );
    let quote_bar_sz = p_bdr_left_sz(quote_p).unwrap_or(0);
    assert!(
        quote_bar_sz >= 24,
        "letter quote bar must be 4px (sz 24 eighths), not Word hairline, got {quote_bar_sz}: {quote_p}"
    );
    assert!(
        quote_p.contains(r#"<w:color w:val="134E4A"/>"#)
            && !quote_p.contains(r#"w:val="nil""#)
            && !quote_p.contains(r#"w:color="auto""#)
            && !quote_p.contains(r#"w:color="CCC""#)
            && !quote_p.contains(r#"w:sz="6""#),
        "letter quote bar/text must stay letter teal, not Word auto/gray/hairline: {quote_p}"
    );
    assert_eq!(
        code_before, 144,
        "letter code_block before must be letter.css 0.6rem (144 twips), not Word 6pt: {code_p}"
    );
    assert!(
        !code_p.contains(r#"w:before="120""#) && !code_p.contains(r#"w:before="160""#),
        "Word 6pt / Normal 8pt before the prove fence vs WeasyPrint 0.6rem: {code_p}"
    );
    assert_eq!(
        code_after, 240,
        "letter code_block after must be letter.css 1rem (240 twips), not Word 8pt: {code_p}"
    );
    assert!(
        !code_p.contains(r#"w:after="160""#) && !code_p.contains(r#"w:after="80""#),
        "Word Normal 8pt / jammed IR after the prove fence vs WeasyPrint 1rem: {code_p}"
    );
    let dock_after = spacing_after_twips(dock_p).unwrap_or(0);
    let bay_after = spacing_after_twips(bay_p).unwrap_or(0);
    let last_task_after = spacing_after_twips(last_task_p).unwrap_or(0);
    let last_num_after = spacing_after_twips(last_num_p).unwrap_or(0);
    assert_eq!(
        dock_after, 60,
        "parent of nested Bay 4 must keep letter 0.25rem (60 twips), not sibling 0.35rem, got {dock_after}: {dock_p}"
    );
    assert_eq!(
        bay_after, 84,
        "nested list items must keep letter 0.35rem (84 twips), not jammed 0 or body 8pt, got {bay_after}: {bay_p}"
    );
    assert_eq!(
        last_task_after, 240,
        "last task item must keep letter 1rem (240 twips), not Word 8pt, got {last_task_after}: {last_task_p}"
    );
    assert_eq!(
        last_num_after, 240,
        "last decimal item must keep letter 1rem (240 twips), not Word 8pt, got {last_num_after}: {last_num_p}"
    );
    assert!(
        document_xml.contains(r#"w:val="ListParagraph""#),
        "letter lists must use Word ListParagraph: {document_xml}"
    );
    assert_eq!(
        (ind_left_twips(dock_p), ind_hanging_twips(dock_p)),
        (Some(300), Some(240)),
        "letter level-0 list indent must be 1.25rem/1rem, not Word 0.5in: {dock_p}"
    );
    assert_eq!(
        (ind_left_twips(bay_p), ind_hanging_twips(bay_p)),
        (Some(600), Some(240)),
        "letter nested list indent must be 2.5rem hanging, not Word 1.0in: {bay_p}"
    );
    assert_eq!(
        num_id(dock_p),
        Some(4),
        "letter - [x] Receiving dock must be a Word checked checkbox, not a disc: {dock_p}"
    );
    assert_eq!(
        num_id(last_task_p),
        Some(3),
        "letter - [ ] Replay must be a Word empty checkbox, not [ ]: {last_task_p}"
    );
    assert_eq!(
        num_id(bay_p),
        Some(1),
        "nested Bay 4 under a task must stay a disc bullet: {bay_p}"
    );
    assert_eq!(
        num_id(last_num_p),
        Some(2),
        "letter decimal list must stay Word numbering, not a task box: {last_num_p}"
    );
    assert!(
        !dock_p.contains("[x]") && !last_task_p.contains("[ ]") && !last_task_p.contains("[x]"),
        "GFM [x]/[ ] must not leak into Word task items: {dock_p} {last_task_p}"
    );

    let quote_style = style_block(&styles_xml, "Quote").expect("Quote style");
    let code_style = style_block(&styles_xml, "CodeBlock").expect("CodeBlock style");
    let list_style = style_block(&styles_xml, "ListParagraph").expect("ListParagraph style");
    assert!(
        (120..=240).contains(&spacing_before_twips(quote_style).unwrap_or(0)),
        "Quote style before must stay 6–12pt: {quote_style}"
    );
    assert_eq!(
        spacing_after_twips(quote_style),
        Some(240),
        "Quote style after must stay letter.css 1rem, not Word Normal 8pt: {quote_style}"
    );
    assert!(
        ind_left_twips(quote_style).unwrap_or(0) >= 720,
        "Quote style indent must stay Word 0.5in: {quote_style}"
    );
    assert!(
        quote_style.contains("<w:pBdr>")
            && quote_style.contains("<w:left ")
            && quote_style.contains(r#"w:sz="24""#)
            && quote_style.contains(r#"w:color="134E4A""#)
            && quote_style.contains(r#"<w:color w:val="134E4A"/>"#),
        "Quote style must keep letter.html 4px #134e4a left bar + teal type: {quote_style}"
    );
    assert!(
        p_bdr_left_sz(quote_style).unwrap_or(0) >= 24,
        "Quote style bar must stay 4px (sz 24), got {:?}: {quote_style}",
        p_bdr_left_sz(quote_style)
    );
    assert!(
        !quote_style.contains(r#"w:val="nil""#)
            && !quote_style.contains(r#"w:color="auto""#)
            && !quote_style.contains(r#"w:sz="6""#),
        "UA/reset/hairline Quote is not WeasyPrint letter input: {quote_style}"
    );
    assert_eq!(
        spacing_before_twips(code_style),
        Some(144),
        "CodeBlock style before must stay letter.css 0.6rem, not Word 6pt: {code_style}"
    );
    assert_eq!(
        spacing_after_twips(code_style),
        Some(240),
        "CodeBlock style after must stay letter.css 1rem, not Word Normal 8pt: {code_style}"
    );
    assert_eq!(
        ind_left_twips(list_style),
        Some(300),
        "ListParagraph style indent must stay letter 1.25rem, not Word 0.5in: {list_style}"
    );
    assert_eq!(
        spacing_after_twips(list_style),
        Some(84),
        "ListParagraph style after must stay letter 0.35rem, not Word contextualSpacing 0: {list_style}"
    );
    assert!(
        !list_style.contains("<w:contextualSpacing"),
        "Word contextualSpacing jams consecutive li vs WeasyPrint 0.35rem: {list_style}"
    );

    let hr_style = style_block(&styles_xml, "HorizontalLine").expect("HorizontalLine style");
    assert!(
        hr_style.contains(r#"w:val="single""#) && hr_style.contains(r#"w:color="134E4A""#),
        "HorizontalLine must be a teal Word pBdr, not missing: {hr_style}"
    );
    let hr_sz = p_bdr_bottom_sz(hr_style).expect("HorizontalLine w:sz");
    assert!(
        hr_sz >= 18,
        "HorizontalLine must be letter 3px (sz 18 eighths), not a hairline, got {hr_sz}: {hr_style}"
    );
    let hr_style_before = spacing_before_twips(hr_style).unwrap_or(0);
    let hr_style_after = spacing_after_twips(hr_style).unwrap_or(0);
    assert!(
        hr_style_before >= 264 && hr_style_after >= 264,
        "HorizontalLine must keep letter 1.1rem (264 twips), got {hr_style_before}/{hr_style_after}: {hr_style}"
    );
    assert!(
        document_xml.contains(r#"w:val="HorizontalLine""#),
        "letter --- must emit Word HorizontalLine: {document_xml}"
    );
    let hr_p = paragraph_containing(&document_xml, r#"w:val="HorizontalLine""#)
        .expect("HorizontalLine paragraph");
    assert!(
        hr_p.contains("<w:pBdr>") && hr_p.contains("<w:bottom "),
        "Word paints --- as a paragraph bottom border: {hr_p}"
    );
    assert!(
        p_bdr_bottom_sz(hr_p).unwrap_or(0) >= 18,
        "letter hr must stay 3px (sz 18): {hr_p}"
    );
    assert!(
        spacing_before_twips(hr_p).unwrap_or(0) >= 264
            && spacing_after_twips(hr_p).unwrap_or(0) >= 264,
        "letter hr must keep 1.1rem, not jammed against the table: {hr_p}"
    );

    let link_style = style_block(&styles_xml, "Hyperlink").expect("Hyperlink style");
    assert!(
        link_style.contains(r#"w:val="0D9488""#) && link_style.contains("<w:u "),
        "Hyperlink style must be letter teal + underline: {link_style}"
    );
    let link_p = paragraph_containing(&document_xml, "DocAgent").expect("DocAgent link paragraph");
    assert!(
        link_p.contains("<w:hyperlink ") && link_p.contains(r#"r:id=""#),
        "letter link must be w:hyperlink, not missing body text: {link_p}"
    );
    assert!(
        link_p.contains(r#"w:val="Hyperlink""#)
            && link_p.contains(r#"<w:u w:val="single"/>"#)
            && link_p.contains(r#"w:val="0D9488""#),
        "letter link must stay underlined teal so Word paints it: {link_p}"
    );
    assert!(
        link_p.contains(">DocAgent</w:t>"),
        "letter link display text must not vanish: {link_p}"
    );

    let flow_p = paragraph_containing(&document_xml, "three hashes")
        .expect("letter mark paragraph");
    assert!(
        flow_p.contains(">prove</w:t>") && flow_p.contains(r#"w:val="Code""#),
        "letter inline `prove` must be a Word Code run, not plain text: {flow_p}"
    );
    assert!(
        flow_p.contains("Consolas")
            && flow_p.contains(r#"w:val="134E4A""#)
            && flow_p.contains(r#"w:fill="CCFBF1""#),
        "letter inline code must be a teal Consolas chip: {flow_p}"
    );
    assert!(
        bdr_space_for_color(flow_p, "CCFBF1").unwrap_or(0) >= 4,
        "letter inline code must keep Word rPr padding (.1em/.35em), got {:?}: {flow_p}",
        bdr_space_for_color(flow_p, "CCFBF1")
    );
    assert!(
        flow_p.contains(">three hashes</w:t>"),
        "letter ==three hashes== must stay in the body: {flow_p}"
    );
    assert!(
        flow_p.contains(r#"w:val="Mark""#)
            && flow_p.contains(r#"<w:highlight w:val="yellow"/>"#)
            && flow_p.contains(r#"w:fill="FDE68A""#),
        "letter mark must be a visible Word highlight + letter amber shading: {flow_p}"
    );
    assert!(
        bdr_space_for_color(flow_p, "FDE68A").unwrap_or(0) >= 2,
        "letter mark must keep Word rPr padding (.05em/.2em), got {:?}: {flow_p}",
        bdr_space_for_color(flow_p, "FDE68A")
    );

    let code_char = style_block(&styles_xml, "Code").expect("Code character style");
    assert!(
        code_char.contains("Consolas")
            && code_char.contains(r#"w:val="134E4A""#)
            && code_char.contains(r#"w:fill="CCFBF1""#),
        "Code style must stay letter teal Consolas chip: {code_char}"
    );
    assert!(
        bdr_space_for_color(code_char, "CCFBF1").unwrap_or(0) >= 4,
        "Code style pad must stay letter .1em/.35em: {code_char}"
    );
    assert_eq!(
        style_sz(code_char).expect("Code w:sz"),
        20,
        "Code style must stay Word 10pt (.92em of 11pt): {code_char}"
    );
    let mark_char = style_block(&styles_xml, "Mark").expect("Mark character style");
    assert!(
        mark_char.contains(r#"w:val="yellow""#) && mark_char.contains(r#"w:fill="FDE68A""#),
        "Mark style must stay Word highlight + letter amber: {mark_char}"
    );
    assert!(
        bdr_space_for_color(mark_char, "FDE68A").unwrap_or(0) >= 2,
        "Mark style pad must stay letter .05em/.2em: {mark_char}"
    );

    let guess_p = paragraph_containing(&document_xml, "guess").expect("letter strike paragraph");
    assert!(
        guess_p.contains(">guess</w:t>")
            && guess_p.contains(r#"w:val="Strike""#)
            && guess_p.contains("<w:strike/>"),
        "letter ~~guess~~ must be Strike + w:strike so Word paints the line, not missing: {guess_p}"
    );
    assert!(
        !guess_p.contains(r#"<w:strike w:val="none"/>"#)
            && !guess_p.contains(r#"<w:strike w:val="false"/>"#),
        "letter strike must not reset to a missing line: {guess_p}"
    );
    let confirm_p = paragraph_containing(&document_xml, "09:00 local")
        .expect("letter underline paragraph");
    assert!(
        confirm_p.contains(">09:00 local</w:t>")
            && confirm_p.contains(r#"w:val="Underline""#)
            && confirm_p.contains(r#"<w:u w:val="single"/>"#),
        "letter __09:00 local__ must be Underline + w:u so Word paints the rule, not missing: {confirm_p}"
    );
    assert!(
        !confirm_p.contains(r#"<w:u w:val="none"/>"#),
        "letter underline must not reset to a missing rule: {confirm_p}"
    );
    let strike_char = style_block(&styles_xml, "Strike").expect("Strike character style");
    assert!(
        strike_char.contains("<w:strike/>") && !strike_char.contains(r#"w:val="none""#),
        "Strike style must stay Word strikethrough: {strike_char}"
    );
    let under_char = style_block(&styles_xml, "Underline").expect("Underline character style");
    assert!(
        under_char.contains(r#"<w:u w:val="single"/>"#) && !under_char.contains(r#"w:val="none""#),
        "Underline style must stay Word Single: {under_char}"
    );

    let a_run = run_containing(flow_p, ">A</w:t>").expect("PDF/^A^ run");
    let two_run = run_containing(flow_p, ">2</w:t>").expect("H~2~O run");
    assert!(
        a_run.contains(r#"<w:rStyle w:val="Superscript"/>"#)
            && a_run.contains(r#"<w:vertAlign w:val="superscript"/>"#)
            && a_run.contains(">A</w:t>"),
        "letter PDF/^A^ must be Superscript + w:vertAlign so Word raises the A, not missing: {a_run}"
    );
    assert!(
        !a_run.contains(r#"w:val="baseline""#) && !a_run.contains(r#"w:val="subscript""#),
        "letter superscript must not reset to baseline or subscript: {a_run}"
    );
    assert!(
        two_run.contains(r#"<w:rStyle w:val="Subscript"/>"#)
            && two_run.contains(r#"<w:vertAlign w:val="subscript"/>"#)
            && two_run.contains(">2</w:t>"),
        "letter H~2~O must be Subscript + w:vertAlign so Word lowers the 2, not missing: {two_run}"
    );
    assert!(
        !two_run.contains(r#"w:val="baseline""#) && !two_run.contains(r#"w:val="superscript""#),
        "letter subscript must not reset to baseline or superscript: {two_run}"
    );
    let super_char = style_block(&styles_xml, "Superscript").expect("Superscript character style");
    assert!(
        super_char.contains(r#"<w:vertAlign w:val="superscript"/>"#)
            && !super_char.contains(r#"w:val="baseline""#)
            && !super_char.contains(r#"w:val="subscript""#),
        "Superscript style must stay Word Font Effects Super: {super_char}"
    );
    let sub_char = style_block(&styles_xml, "Subscript").expect("Subscript character style");
    assert!(
        sub_char.contains(r#"<w:vertAlign w:val="subscript"/>"#)
            && !sub_char.contains(r#"w:val="baseline""#)
            && !sub_char.contains(r#"w:val="superscript""#),
        "Subscript style must stay Word Font Effects Sub: {sub_char}"
    );
    let bay_sub = run_containing(bay_p, ">2</w:t>").expect("Bay 4 H~2~O run");
    assert!(
        bay_sub.contains(r#"<w:vertAlign w:val="subscript"/>"#),
        "nested Bay 4 H~2~O must keep w:vertAlign subscript: {bay_sub}"
    );
    let link_super = run_containing(link_p, ">A</w:t>").expect("closing PDF/^A^ run");
    let link_sub = run_containing(link_p, ">2</w:t>").expect("closing H~2~O run");
    assert!(
        link_super.contains(r#"<w:vertAlign w:val="superscript"/>"#)
            && link_sub.contains(r#"<w:vertAlign w:val="subscript"/>"#),
        "closing PDF/^A^ and H~2~O must stay w:vertAlign: {link_p}"
    );

    let mut numbering_xml = String::new();
    {
        let mut file = zip
            .by_name("word/numbering.xml")
            .expect("word/numbering.xml");
        file.read_to_string(&mut numbering_xml).unwrap();
    }
    assert!(
        numbering_xml.contains(r#"w:left="300" w:hanging="240""#),
        "letter numbering must keep 1.25rem/1rem hanging, not Word 0.5in: {numbering_xml}"
    );
    assert!(
        numbering_xml.contains(r#"w:left="600" w:hanging="240""#),
        "letter nested numbering must keep 2.5rem hanging, not Word 1.0in: {numbering_xml}"
    );
    assert!(
        !numbering_xml.contains(r#"w:left="720" w:hanging="360""#)
            && !numbering_xml.contains(r#"w:left="1440" w:hanging="360""#),
        "Word 0.5in/0.25in hanging is a sparse gutter vs WeasyPrint 1.25rem: {numbering_xml}"
    );
    assert!(
        numbering_xml.contains('\u{2610}') && numbering_xml.contains('\u{2612}'),
        "letter task numbering must be Word ☐/☒ checkboxes, not a missing disc: {numbering_xml}"
    );
    assert!(
        numbering_xml.contains("Segoe UI Symbol") && numbering_xml.contains("MS Gothic"),
        "letter task checkboxes must use Word checkbox fonts: {numbering_xml}"
    );
    assert!(
        numbering_xml.contains(r#"<w:color w:val="134E4A"/>"#),
        "letter task checkboxes must stay letter teal: {numbering_xml}"
    );
    assert!(
        numbering_xml.contains(r#"<w:sz w:val="24"/>"#)
            && numbering_xml.contains(r#"<w:szCs w:val="24"/>"#),
        "letter task checkboxes must stay 12pt (sz 24), not a tofu-scale glyph: {numbering_xml}"
    );
    assert!(
        !numbering_xml.contains("[x]") && !numbering_xml.contains("[ ]"),
        "letter numbering must not use ASCII task boxes: {numbering_xml}"
    );

    let mut rels_xml = String::new();
    {
        let mut file = zip
            .by_name("word/_rels/document.xml.rels")
            .expect("word/_rels/document.xml.rels");
        file.read_to_string(&mut rels_xml).unwrap();
    }
    assert!(
        rels_xml.contains("/relationships/hyperlink")
            && rels_xml.contains("https://github.com/kevin9327/docagent")
            && rels_xml.contains(r#"TargetMode="External""#),
        "letter hyperlink rel must be an external Target: {rels_xml}"
    );
    assert!(
        rels_xml.contains("/relationships/image"),
        "letter image rel must stay so a:blip paints: {rels_xml}"
    );

    let mark = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/assets/mark.png"),
    )
    .expect("docs/assets/mark.png");
    assert!(mark.starts_with(PNG_MAGIC), "fixture must be PNG");

    let mut found_png = false;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).unwrap();
        let name = file.name().replace('\\', "/");
        if !name.starts_with("word/media/") || name.ends_with('/') {
            continue;
        }
        let mut stored = Vec::new();
        file.read_to_end(&mut stored).unwrap();
        if stored.starts_with(PNG_MAGIC) {
            assert_eq!(stored, mark, "{name} must be mark.png bytes");
            found_png = true;
            break;
        }
    }
    assert!(
        found_png,
        "DOCX from letter.md must store PNG bytes under word/media"
    );
}

#[test]
fn letter_md_docx_super_and_sub_like_word() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("PDF/^A^") && text.contains("H~2~O"),
        "letter.md must keep PDF/^A^ and H~2~O"
    );
    assert!(
        text.contains("~~guess~~") && text.contains("- [x] Receiving dock is clear"),
        "letter.md must keep strike and tasks"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    assert!(
        doc.sections[0].body.iter().any(|b| match b {
            Block::Paragraph(p) => p.runs.iter().any(|r| {
                r.style.superscript && matches!(&r.content, RunContent::Text(t) if t == "A")
            }),
            _ => false,
        }),
        "letter.md IR must keep PDF/^A^ as superscript"
    );
    assert!(
        doc.sections[0].body.iter().any(|b| match b {
            Block::Paragraph(p) => p.runs.iter().any(|r| {
                r.style.subscript && matches!(&r.content, RunContent::Text(t) if t == "2")
            }),
            _ => false,
        }),
        "letter.md IR must keep H~2~O as subscript"
    );
    assert!(
        doc.sections[0].body.iter().any(|b| match b {
            Block::Paragraph(p) => p.runs.iter().any(|r| {
                r.style.strike && matches!(&r.content, RunContent::Text(t) if t == "guess")
            }),
            _ => false,
        }),
        "strike must not regress"
    );
    assert!(
        doc.sections[0].body.iter().any(|b| match b {
            Block::Paragraph(p) => p
                .numbering
                .as_ref()
                .is_some_and(|n| n.format == Some(NumberFormat::Task)),
            _ => false,
        }),
        "tasks must not regress"
    );

    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut styles_xml = String::new();
    zip.by_name("word/styles.xml")
        .expect("word/styles.xml")
        .read_to_string(&mut styles_xml)
        .unwrap();

    let flow_p = paragraph_containing(&document_xml, "three hashes")
        .expect("letter super/sub paragraph");
    let a_run = run_containing(flow_p, ">A</w:t>").expect("PDF/^A^ run");
    let two_run = run_containing(flow_p, ">2</w:t>").expect("H~2~O run");
    assert!(
        a_run.contains(r#"<w:rStyle w:val="Superscript"/>"#)
            && a_run.contains(r#"<w:vertAlign w:val="superscript"/>"#),
        "letter PDF/^A^ must be Superscript + w:vertAlign so Word raises the A, not missing: {a_run}"
    );
    assert!(
        two_run.contains(r#"<w:rStyle w:val="Subscript"/>"#)
            && two_run.contains(r#"<w:vertAlign w:val="subscript"/>"#),
        "letter H~2~O must be Subscript + w:vertAlign so Word lowers the 2, not missing: {two_run}"
    );
    let super_char = style_block(&styles_xml, "Superscript").expect("Superscript");
    assert!(
        super_char.contains(r#"<w:vertAlign w:val="superscript"/>"#)
            && !super_char.contains(r#"w:val="baseline""#),
        "Superscript must be Word Font Effects Super: {super_char}"
    );
    let sub_char = style_block(&styles_xml, "Subscript").expect("Subscript");
    assert!(
        sub_char.contains(r#"<w:vertAlign w:val="subscript"/>"#)
            && !sub_char.contains(r#"w:val="baseline""#),
        "Subscript must be Word Font Effects Sub: {sub_char}"
    );
    assert!(
        !super_char.contains("100%") && !sub_char.contains("100%"),
        "super/sub must not stay full-size baseline: {super_char} {sub_char}"
    );

    let guess_p = paragraph_containing(&document_xml, "guess").expect("letter strike paragraph");
    assert!(
        guess_p.contains(r#"w:val="Strike""#) && guess_p.contains("<w:strike/>"),
        "strike must not regress: {guess_p}"
    );
    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains("<w:tcBorders>"),
        "th teal fill must not regress: {hop_tc}"
    );
    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    assert_eq!(
        num_id(dock_p),
        Some(4),
        "checked task numId must not regress: {dock_p}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
}

#[test]
fn letter_md_docx_quote_bar_like_word() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("> Replay is evidence. Same fonts, same bytes."),
        "letter.md must keep the blockquote Word paints with a 4px left bar"
    );
    assert!(
        text.contains("PDF/^A^") && text.contains("H~2~O"),
        "letter.md must keep PDF/^A^ and H~2~O"
    );
    assert!(
        text.contains("- [x] Receiving dock is clear"),
        "letter.md must keep tasks"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    assert!(
        doc.sections[0].body.iter().any(|b| match b {
            Block::Paragraph(p) => p.quote && p.plain_text().contains("Replay is evidence"),
            _ => false,
        }),
        "letter.md IR must keep the blockquote"
    );

    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut styles_xml = String::new();
    zip.by_name("word/styles.xml")
        .expect("word/styles.xml")
        .read_to_string(&mut styles_xml)
        .unwrap();

    let quote_p = paragraph_containing(&document_xml, "Replay is evidence")
        .expect("blockquote paragraph");
    assert!(
        quote_p.contains(r#"w:val="Quote""#)
            && quote_p.contains("Replay is evidence. Same fonts, same bytes."),
        "letter blockquote must use Quote: {quote_p}"
    );
    assert_eq!(
        spacing_after_twips(quote_p),
        Some(240),
        "letter blockquote after must stay letter.css 1rem (240 twips), not Word 8pt: {quote_p}"
    );
    assert!(
        ind_left_twips(quote_p).unwrap_or(0) >= 720,
        "letter quote indent must stay Word 0.5in, got {:?}: {quote_p}",
        ind_left_twips(quote_p)
    );
    assert!(
        quote_p.contains("<w:pBdr>") && quote_p.contains("<w:left "),
        "Word paints blockquote as a paragraph left border: {quote_p}"
    );
    let quote_bar_sz = p_bdr_left_sz(quote_p).unwrap_or(0);
    assert!(
        quote_bar_sz >= 24,
        "Quote bar must be letter.html 4px (sz 24 eighths), not Word hairline, got {quote_bar_sz}: {quote_p}"
    );
    assert!(
        quote_p.contains(r#"w:color="134E4A""#),
        "Quote w:pBdr left must match letter.html 4px #134e4a, not Word gray: {quote_p}"
    );
    assert!(
        quote_p.contains(r#"<w:color w:val="134E4A"/>"#),
        "Quote text must stay letter.html #134e4a like the 4px bar: {quote_p}"
    );
    assert!(
        !quote_p.contains(r#"w:val="nil""#)
            && !quote_p.contains(r#"w:color="auto""#)
            && !quote_p.contains(r#"w:color="CCC""#)
            && !quote_p.contains(r#"w:color="999999""#)
            && !quote_p.contains(r#"w:sz="6""#)
            && !quote_p.contains(r#"w:sz="8""#),
        "UA/reset/hairline/gray quote bar is not WeasyPrint letter input: {quote_p}"
    );

    let quote_style = style_block(&styles_xml, "Quote").expect("Quote style");
    assert!(
        quote_style.contains("<w:pBdr>")
            && quote_style.contains("<w:left ")
            && quote_style.contains(r#"w:sz="24""#)
            && quote_style.contains(r#"w:color="134E4A""#)
            && quote_style.contains(r#"<w:color w:val="134E4A"/>"#)
            && p_bdr_left_sz(quote_style).unwrap_or(0) >= 24,
        "Quote style must keep letter.html 4px #134e4a left bar: {quote_style}"
    );

    let flow_p = paragraph_containing(&document_xml, "three hashes")
        .expect("letter super/sub paragraph");
    let a_run = run_containing(flow_p, ">A</w:t>").expect("PDF/^A^ run");
    let two_run = run_containing(flow_p, ">2</w:t>").expect("H~2~O run");
    assert!(
        a_run.contains(r#"<w:vertAlign w:val="superscript"/>"#)
            && two_run.contains(r#"<w:vertAlign w:val="subscript"/>"#),
        "quote bar must keep Tsuper/Tsub vertAlign: {a_run} {two_run}"
    );
    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains("<w:tcBorders>"),
        "quote bar must keep th teal fill: {hop_tc}"
    );
    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    assert_eq!(
        num_id(dock_p),
        Some(4),
        "quote bar must keep checked task numId 4: {dock_p}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
}

#[test]
fn letter_md_docx_quote_after_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("> Replay is evidence. Same fonts, same bytes."),
        "letter.md must keep the blockquote Word paints with letter.css 1rem after"
    );
    assert!(
        text.contains("```") && text.contains("docagent prove examples/letter.md"),
        "letter.md must keep the prove fence after the quote"
    );
    assert!(
        text.contains("6 September 2026"),
        "letter.md must keep the body date"
    );
    assert!(
        text.contains("- [x] Receiving dock is clear") && text.contains("  - Bay 4"),
        "letter.md must keep nested Bay 4"
    );
    assert!(
        text.contains("- [ ] Replay the convert instead of hoping")
            && text.contains("3. Compare the capsule"),
        "letter.md must keep last-list 1rem after"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    assert!(
        doc.sections[0].body.iter().any(|b| match b {
            Block::Paragraph(p) => p.quote && p.plain_text().contains("Replay is evidence"),
            _ => false,
        }),
        "letter.md IR must keep the blockquote"
    );

    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut styles_xml = String::new();
    zip.by_name("word/styles.xml")
        .expect("word/styles.xml")
        .read_to_string(&mut styles_xml)
        .unwrap();

    let quote_p = paragraph_containing(&document_xml, "Replay is evidence")
        .expect("blockquote paragraph");
    assert_eq!(
        spacing_after_twips(quote_p),
        Some(240),
        "letter blockquote after must be letter.css 1rem (240 twips), not Word 8pt: {quote_p}"
    );
    assert!(
        !quote_p.contains(r#"w:after="160""#)
            && !quote_p.contains(r#"w:after="80""#)
            && !quote_p.contains(r#"w:after="168""#),
        "Word Normal 8pt / jammed IR / Harbor 0.7rem after the quote vs WeasyPrint 1rem: {quote_p}"
    );
    assert!(
        quote_p.contains(r#"w:val="Quote""#)
            && quote_p.contains("<w:pBdr>")
            && quote_p.contains("<w:left ")
            && quote_p.contains(r#"w:color="134E4A""#)
            && p_bdr_left_sz(quote_p).unwrap_or(0) >= 24,
        "quote after 1rem must keep the 4px #134e4a bar: {quote_p}"
    );
    assert!(
        (120..=240).contains(&spacing_before_twips(quote_p).unwrap_or(0)),
        "quote after 1rem must keep letter 0.5rem before: {quote_p}"
    );
    let quote_style = style_block(&styles_xml, "Quote").expect("Quote style");
    assert_eq!(
        spacing_after_twips(quote_style),
        Some(240),
        "Quote style after must stay letter.css 1rem so style-only Quote is not Normal 8pt: {quote_style}"
    );
    assert!(
        quote_style.contains("<w:pBdr>")
            && quote_style.contains(r#"w:sz="24""#)
            && quote_style.contains(r#"w:color="134E4A""#),
        "quote after must keep Quote style 4px bar: {quote_style}"
    );

    let body_p = paragraph_containing(&document_xml, "6 September 2026")
        .expect("body date paragraph");
    assert_eq!(
        spacing_after_twips(body_p),
        Some(168),
        "quote 1rem after must keep body letter.css 0.7rem: {body_p}"
    );
    assert_eq!(
        spacing_attr(body_p, "w:line"),
        Some(276),
        "quote after must keep Word 1.15 (276): {body_p}"
    );
    let mark_p = harbor_paragraph(&document_xml).expect("paragraph wrapping the Harbor a:blip");
    assert_eq!(
        spacing_before_twips(mark_p),
        Some(144),
        "quote after must keep Harbor before-mark 0.6rem: {mark_p}"
    );
    assert_eq!(
        spacing_after_twips(mark_p),
        Some(168),
        "quote after must keep Harbor after-mark 0.7rem: {mark_p}"
    );

    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    let last_task_p = paragraph_containing(&document_xml, "Replay the convert instead of hoping")
        .expect("last task list item");
    let last_num_p = paragraph_containing(&document_xml, "Compare the capsule")
        .expect("last decimal list item");
    assert_eq!(
        spacing_after_twips(dock_p),
        Some(60),
        "quote after must keep nested 0.25rem: {dock_p}"
    );
    assert_eq!(
        spacing_after_twips(last_task_p),
        Some(240),
        "quote after must keep last task 1rem: {last_task_p}"
    );
    assert_eq!(
        spacing_after_twips(last_num_p),
        Some(240),
        "quote after must keep last decimal 1rem: {last_num_p}"
    );
    assert_eq!(
        (ind_left_twips(dock_p), ind_hanging_twips(dock_p)),
        (Some(300), Some(240)),
        "quote after must keep 1.25rem/1rem hanging: {dock_p}"
    );
    assert_eq!(
        num_id(dock_p),
        Some(4),
        "quote after must keep checked task numId 4: {dock_p}"
    );

    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains(r#"<w:vAlign w:val="top"/>"#),
        "quote after must keep th teal fill + valign top: {hop_tc}"
    );
    let h1_p = paragraph_containing(&document_xml, r#"w:val="Heading1""#)
        .expect("Heading1 paragraph");
    let h2_p = paragraph_containing(&document_xml, r#"w:val="Heading2""#)
        .expect("Heading2 paragraph");
    assert_eq!(
        spacing_attr(h1_p, "w:line"),
        Some(288),
        "quote after must keep heading 1.2 leading: {h1_p}"
    );
    assert!(
        !h1_p.contains("<w:sz ") && !h2_p.contains("<w:sz "),
        "quote after must not reintroduce markdown CSS w:sz: {h1_p} {h2_p}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
}

#[test]
fn letter_md_docx_code_block_after_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("```") && text.contains("docagent prove examples/letter.md"),
        "letter.md must keep the prove fence Word paints with letter.css 1rem after"
    );
    assert!(
        text.contains("> Replay is evidence. Same fonts, same bytes."),
        "letter.md must keep the blockquote before the prove fence"
    );
    assert!(
        text.contains("| Hop | File | Proof |"),
        "letter.md must keep the hop table after the prove fence"
    );
    assert!(
        text.contains("6 September 2026"),
        "letter.md must keep the body date"
    );
    assert!(
        text.contains("- [x] Receiving dock is clear") && text.contains("  - Bay 4"),
        "letter.md must keep nested Bay 4"
    );
    assert!(
        text.contains("- [ ] Replay the convert instead of hoping")
            && text.contains("3. Compare the capsule"),
        "letter.md must keep last-list 1rem after"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    assert!(
        doc.sections[0].body.iter().any(|b| match b {
            Block::Paragraph(p) => p.code_block && p.plain_text().contains("docagent prove"),
            _ => false,
        }),
        "letter.md IR must keep the prove code_block"
    );

    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut styles_xml = String::new();
    zip.by_name("word/styles.xml")
        .expect("word/styles.xml")
        .read_to_string(&mut styles_xml)
        .unwrap();

    let code_p = paragraph_containing(&document_xml, "docagent prove")
        .expect("code_block paragraph");
    assert_eq!(
        spacing_after_twips(code_p),
        Some(240),
        "letter prove fence after must be letter.css 1rem (240 twips), not Word 8pt: {code_p}"
    );
    assert!(
        !code_p.contains(r#"w:after="160""#)
            && !code_p.contains(r#"w:after="80""#)
            && !code_p.contains(r#"w:after="168""#),
        "Word Normal 8pt / jammed IR / Harbor 0.7rem after the prove fence vs WeasyPrint 1rem: {code_p}"
    );
    assert!(
        code_p.contains(r#"w:val="CodeBlock""#)
            && code_p.contains(r#"w:fill="CCFBF1""#)
            && code_p.contains("<w:pBdr>")
            && p_bdr_top_sz(code_p).unwrap_or(0) >= 72
            && p_bdr_bottom_sz(code_p).unwrap_or(0) >= 72,
        "code after 1rem must keep the teal Consolas wash + 0.75rem Y pad: {code_p}"
    );
    assert_eq!(
        (ind_left_twips(code_p), ind_right_twips(code_p)),
        (Some(240), Some(240)),
        "code after 1rem must keep letter pre 1rem X pad: {code_p}"
    );
    let code_style = style_block(&styles_xml, "CodeBlock").expect("CodeBlock style");
    assert_eq!(
        spacing_after_twips(code_style),
        Some(240),
        "CodeBlock style after must stay letter.css 1rem so style-only CodeBlock is not Normal 8pt: {code_style}"
    );
    assert!(
        code_style.contains("<w:pBdr>")
            && p_bdr_top_sz(code_style).unwrap_or(0) >= 72
            && p_bdr_bottom_sz(code_style).unwrap_or(0) >= 72
            && code_style.contains(r#"w:color="CCFBF1""#),
        "code after must keep CodeBlock style 0.75rem Y pad: {code_style}"
    );
    assert_eq!(
        spacing_before_twips(code_p),
        Some(144),
        "code after 1rem must keep letter.css 0.6rem before: {code_p}"
    );

    let quote_p = paragraph_containing(&document_xml, "Replay is evidence")
        .expect("blockquote paragraph");
    assert_eq!(
        spacing_after_twips(quote_p),
        Some(240),
        "code 1rem after must keep quote 1rem: {quote_p}"
    );
    assert!(
        quote_p.contains(r#"w:val="Quote""#)
            && quote_p.contains("<w:pBdr>")
            && quote_p.contains("<w:left ")
            && quote_p.contains(r#"w:color="134E4A""#)
            && p_bdr_left_sz(quote_p).unwrap_or(0) >= 24,
        "code after 1rem must keep the 4px #134e4a bar: {quote_p}"
    );

    let body_p = paragraph_containing(&document_xml, "6 September 2026")
        .expect("body date paragraph");
    assert_eq!(
        spacing_after_twips(body_p),
        Some(168),
        "code 1rem after must keep body letter.css 0.7rem: {body_p}"
    );
    assert_eq!(
        spacing_attr(body_p, "w:line"),
        Some(276),
        "code after must keep Word 1.15 (276): {body_p}"
    );
    let mark_p = harbor_paragraph(&document_xml).expect("paragraph wrapping the Harbor a:blip");
    assert_eq!(
        spacing_before_twips(mark_p),
        Some(144),
        "code after must keep Harbor before-mark 0.6rem: {mark_p}"
    );
    assert_eq!(
        spacing_after_twips(mark_p),
        Some(168),
        "code after must keep Harbor after-mark 0.7rem: {mark_p}"
    );

    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    let last_task_p = paragraph_containing(&document_xml, "Replay the convert instead of hoping")
        .expect("last task list item");
    let last_num_p = paragraph_containing(&document_xml, "Compare the capsule")
        .expect("last decimal list item");
    assert_eq!(
        spacing_after_twips(dock_p),
        Some(60),
        "code after must keep nested 0.25rem: {dock_p}"
    );
    assert_eq!(
        spacing_after_twips(last_task_p),
        Some(240),
        "code after must keep last task 1rem: {last_task_p}"
    );
    assert_eq!(
        spacing_after_twips(last_num_p),
        Some(240),
        "code after must keep last decimal 1rem: {last_num_p}"
    );
    assert_eq!(
        (ind_left_twips(dock_p), ind_hanging_twips(dock_p)),
        (Some(300), Some(240)),
        "code after must keep 1.25rem/1rem hanging: {dock_p}"
    );
    assert_eq!(
        num_id(dock_p),
        Some(4),
        "code after must keep checked task numId 4: {dock_p}"
    );

    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains(r#"<w:vAlign w:val="top"/>"#),
        "code after must keep th teal fill + valign top: {hop_tc}"
    );
    let h1_p = paragraph_containing(&document_xml, r#"w:val="Heading1""#)
        .expect("Heading1 paragraph");
    let h2_p = paragraph_containing(&document_xml, r#"w:val="Heading2""#)
        .expect("Heading2 paragraph");
    assert_eq!(
        spacing_attr(h1_p, "w:line"),
        Some(288),
        "code after must keep heading 1.2 leading: {h1_p}"
    );
    assert!(
        !h1_p.contains("<w:sz ") && !h2_p.contains("<w:sz "),
        "code after must not reintroduce markdown CSS w:sz: {h1_p} {h2_p}"
    );
    let flow_p = paragraph_containing(&document_xml, "three hashes")
        .expect("letter mark paragraph");
    assert!(
        flow_p.contains(">prove</w:t>") && flow_p.contains(r#"w:val="Code""#),
        "code after must keep the inline 10pt prove chip: {flow_p}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
}

#[test]
fn letter_md_docx_code_block_before_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("```") && text.contains("docagent prove examples/letter.md"),
        "letter.md must keep the prove fence Word paints with letter.css 0.6rem before"
    );
    assert!(
        text.contains("> Replay is evidence. Same fonts, same bytes."),
        "letter.md must keep the blockquote before the prove fence"
    );
    assert!(
        text.contains("| Hop | File | Proof |"),
        "letter.md must keep the hop table after the prove fence"
    );
    assert!(
        text.contains("6 September 2026"),
        "letter.md must keep the body date"
    );
    assert!(
        text.contains("- [x] Receiving dock is clear") && text.contains("  - Bay 4"),
        "letter.md must keep nested Bay 4"
    );
    assert!(
        text.contains("- [ ] Replay the convert instead of hoping")
            && text.contains("3. Compare the capsule"),
        "letter.md must keep last-list 1rem after"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    assert!(
        doc.sections[0].body.iter().any(|b| match b {
            Block::Paragraph(p) => p.code_block && p.plain_text().contains("docagent prove"),
            _ => false,
        }),
        "letter.md IR must keep the prove code_block"
    );

    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut styles_xml = String::new();
    zip.by_name("word/styles.xml")
        .expect("word/styles.xml")
        .read_to_string(&mut styles_xml)
        .unwrap();

    let code_p = paragraph_containing(&document_xml, "docagent prove")
        .expect("code_block paragraph");
    assert_eq!(
        spacing_before_twips(code_p),
        Some(144),
        "letter prove fence before must be letter.css 0.6rem (144 twips), not Word 6pt: {code_p}"
    );
    assert!(
        !code_p.contains(r#"w:before="16""#)
            && !code_p.contains(r#"w:before="80""#)
            && !code_p.contains(r#"w:before="120""#)
            && !code_p.contains(r#"w:before="160""#),
        "IR 80 / Word 6pt / Normal 8pt before the prove fence vs WeasyPrint 0.6rem: {code_p}"
    );
    assert_eq!(
        spacing_after_twips(code_p),
        Some(240),
        "code 0.6rem before must keep letter.css 1rem after: {code_p}"
    );
    assert!(
        code_p.contains(r#"w:val="CodeBlock""#)
            && code_p.contains(r#"w:fill="CCFBF1""#)
            && code_p.contains("<w:pBdr>")
            && p_bdr_top_sz(code_p).unwrap_or(0) >= 72
            && p_bdr_bottom_sz(code_p).unwrap_or(0) >= 72,
        "code before 0.6rem must keep the teal Consolas wash + 0.75rem Y pad: {code_p}"
    );
    assert_eq!(
        (ind_left_twips(code_p), ind_right_twips(code_p)),
        (Some(240), Some(240)),
        "code before 0.6rem must keep letter pre 1rem X pad: {code_p}"
    );
    let code_style = style_block(&styles_xml, "CodeBlock").expect("CodeBlock style");
    assert_eq!(
        spacing_before_twips(code_style),
        Some(144),
        "CodeBlock style before must stay letter.css 0.6rem so style-only CodeBlock is not Word 6pt: {code_style}"
    );
    assert_eq!(
        spacing_after_twips(code_style),
        Some(240),
        "code before must keep CodeBlock style 1rem after: {code_style}"
    );

    let quote_p = paragraph_containing(&document_xml, "Replay is evidence")
        .expect("blockquote paragraph");
    assert_eq!(
        spacing_after_twips(quote_p),
        Some(240),
        "code 0.6rem before must keep quote 1rem: {quote_p}"
    );
    assert!(
        quote_p.contains(r#"w:val="Quote""#)
            && quote_p.contains("<w:pBdr>")
            && quote_p.contains("<w:left ")
            && quote_p.contains(r#"w:color="134E4A""#)
            && p_bdr_left_sz(quote_p).unwrap_or(0) >= 24,
        "code before 0.6rem must keep the 4px #134e4a bar: {quote_p}"
    );
    assert!(
        (120..=240).contains(&spacing_before_twips(quote_p).unwrap_or(0)),
        "code 0.6rem before must keep letter 0.5rem quote before: {quote_p}"
    );

    let body_p = paragraph_containing(&document_xml, "6 September 2026")
        .expect("body date paragraph");
    assert_eq!(
        spacing_after_twips(body_p),
        Some(168),
        "code 0.6rem before must keep body letter.css 0.7rem: {body_p}"
    );
    assert_eq!(
        spacing_attr(body_p, "w:line"),
        Some(276),
        "code before must keep Word 1.15 (276): {body_p}"
    );
    let mark_p = harbor_paragraph(&document_xml).expect("paragraph wrapping the Harbor a:blip");
    assert_eq!(
        spacing_before_twips(mark_p),
        Some(144),
        "code before must keep Harbor before-mark 0.6rem: {mark_p}"
    );
    assert_eq!(
        spacing_after_twips(mark_p),
        Some(168),
        "code before must keep Harbor after-mark 0.7rem: {mark_p}"
    );

    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    let last_task_p = paragraph_containing(&document_xml, "Replay the convert instead of hoping")
        .expect("last task list item");
    let last_num_p = paragraph_containing(&document_xml, "Compare the capsule")
        .expect("last decimal list item");
    assert_eq!(
        spacing_after_twips(dock_p),
        Some(60),
        "code before must keep nested 0.25rem: {dock_p}"
    );
    assert_eq!(
        spacing_after_twips(last_task_p),
        Some(240),
        "code before must keep last task 1rem: {last_task_p}"
    );
    assert_eq!(
        spacing_after_twips(last_num_p),
        Some(240),
        "code before must keep last decimal 1rem: {last_num_p}"
    );
    assert_eq!(
        (ind_left_twips(dock_p), ind_hanging_twips(dock_p)),
        (Some(300), Some(240)),
        "code before must keep 1.25rem/1rem hanging: {dock_p}"
    );
    assert_eq!(
        num_id(dock_p),
        Some(4),
        "code before must keep checked task numId 4: {dock_p}"
    );

    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains(r#"<w:vAlign w:val="top"/>"#),
        "code before must keep th teal fill + valign top: {hop_tc}"
    );
    let h1_p = paragraph_containing(&document_xml, r#"w:val="Heading1""#)
        .expect("Heading1 paragraph");
    let h2_p = paragraph_containing(&document_xml, r#"w:val="Heading2""#)
        .expect("Heading2 paragraph");
    assert_eq!(
        spacing_attr(h1_p, "w:line"),
        Some(288),
        "code before must keep heading 1.2 leading: {h1_p}"
    );
    assert!(
        !h1_p.contains("<w:sz ") && !h2_p.contains("<w:sz "),
        "code before must not reintroduce markdown CSS w:sz: {h1_p} {h2_p}"
    );
    let flow_p = paragraph_containing(&document_xml, "three hashes")
        .expect("letter mark paragraph");
    assert!(
        flow_p.contains(">prove</w:t>") && flow_p.contains(r#"w:val="Code""#),
        "code before must keep the inline 10pt prove chip: {flow_p}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
}

#[test]
fn letter_md_docx_body_after_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("6 September 2026") && text.contains("operations desk"),
        "letter.md must keep body copy Word paints with letter.css p 0.7rem after"
    );
    assert!(
        text.contains("# Northwind Freight") && text.contains("## Delivery confirmation"),
        "letter.md must keep H1/H2"
    );
    assert!(
        text.contains("![Harbor mark](docs/assets/mark.png)"),
        "letter.md must keep the Harbor mark under H2"
    );
    assert!(
        text.contains("> Replay is evidence. Same fonts, same bytes."),
        "letter.md must keep the blockquote"
    );
    assert!(
        text.contains("```") && text.contains("docagent prove examples/letter.md"),
        "letter.md must keep the prove fence"
    );
    assert!(
        text.contains("- [x] Receiving dock is clear") && text.contains("  - Bay 4"),
        "letter.md must keep nested Bay 4"
    );
    assert!(
        text.contains("- [ ] Replay the convert instead of hoping")
            && text.contains("3. Compare the capsule"),
        "letter.md must keep last-list 1rem after"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut styles_xml = String::new();
    zip.by_name("word/styles.xml")
        .expect("word/styles.xml")
        .read_to_string(&mut styles_xml)
        .unwrap();

    let body_p = paragraph_containing(&document_xml, "6 September 2026")
        .expect("body date paragraph");
    let dest_p = paragraph_containing(&document_xml, "operations desk")
        .expect("body dest paragraph");
    assert_eq!(
        spacing_after_twips(body_p),
        Some(168),
        "letter body after must be letter.css p 0.7rem (168 twips), not Word 8pt: {body_p}"
    );
    assert_eq!(
        spacing_after_twips(dest_p),
        Some(168),
        "consecutive body after must stay letter.css 0.7rem, not Word 8pt: {dest_p}"
    );
    assert!(
        !body_p.contains(r#"w:after="160""#)
            && !body_p.contains(r#"w:after="40""#)
            && !body_p.contains(r#"w:after="240""#),
        "Word Normal 8pt / jammed IR / last-list 1rem after body vs WeasyPrint 0.7rem: {body_p}"
    );
    assert_eq!(
        spacing_attr(body_p, "w:line"),
        Some(276),
        "body 0.7rem after must keep Word 1.15 (276): {body_p}"
    );
    let normal_style = style_block(&styles_xml, "Normal").expect("Normal style");
    assert_eq!(
        spacing_after_twips(normal_style),
        Some(168),
        "Normal style after must stay letter.css 0.7rem so style-only Normal is not Word 8pt: {normal_style}"
    );
    assert!(
        styles_xml.contains(r#"<w:pPrDefault><w:pPr><w:spacing w:after="168" w:line="276""#),
        "docDefaults after must stay letter.css 0.7rem, not Word 8pt"
    );

    let mark_p = harbor_paragraph(&document_xml).expect("paragraph wrapping the Harbor a:blip");
    assert_eq!(
        spacing_before_twips(mark_p),
        Some(144),
        "body 0.7rem after must keep Harbor before-mark 0.6rem: {mark_p}"
    );
    assert_eq!(
        spacing_after_twips(mark_p),
        Some(168),
        "body 0.7rem after must keep Harbor after-mark 0.7rem: {mark_p}"
    );
    let quote_p = paragraph_containing(&document_xml, "Replay is evidence")
        .expect("blockquote paragraph");
    assert_eq!(
        spacing_after_twips(quote_p),
        Some(240),
        "body 0.7rem after must keep quote 1rem: {quote_p}"
    );
    assert!(
        quote_p.contains(r#"w:val="Quote""#)
            && quote_p.contains("<w:pBdr>")
            && quote_p.contains("<w:left ")
            && quote_p.contains(r#"w:color="134E4A""#)
            && p_bdr_left_sz(quote_p).unwrap_or(0) >= 24,
        "body 0.7rem after must keep the 4px #134e4a bar: {quote_p}"
    );
    let code_p = paragraph_containing(&document_xml, "docagent prove")
        .expect("prove fence");
    assert_eq!(
        spacing_before_twips(code_p),
        Some(144),
        "body 0.7rem after must keep pre 0.6rem before: {code_p}"
    );
    assert_eq!(
        spacing_after_twips(code_p),
        Some(240),
        "body 0.7rem after must keep pre 1rem after: {code_p}"
    );

    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    let last_task_p = paragraph_containing(&document_xml, "Replay the convert instead of hoping")
        .expect("last task list item");
    let last_num_p = paragraph_containing(&document_xml, "Compare the capsule")
        .expect("last decimal list item");
    assert_eq!(
        spacing_after_twips(dock_p),
        Some(60),
        "body 0.7rem after must keep nested 0.25rem: {dock_p}"
    );
    assert_eq!(
        spacing_after_twips(last_task_p),
        Some(240),
        "body 0.7rem after must keep last task 1rem: {last_task_p}"
    );
    assert_eq!(
        spacing_after_twips(last_num_p),
        Some(240),
        "body 0.7rem after must keep last decimal 1rem: {last_num_p}"
    );
    assert_eq!(
        (ind_left_twips(dock_p), ind_hanging_twips(dock_p)),
        (Some(300), Some(240)),
        "body 0.7rem after must keep 1.25rem/1rem hanging: {dock_p}"
    );

    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains(r#"<w:vAlign w:val="top"/>"#),
        "body 0.7rem after must keep th teal fill + valign top: {hop_tc}"
    );
    let h1_p = paragraph_containing(&document_xml, r#"w:val="Heading1""#)
        .expect("Heading1 paragraph");
    let h2_p = paragraph_containing(&document_xml, r#"w:val="Heading2""#)
        .expect("Heading2 paragraph");
    assert_eq!(
        spacing_attr(h1_p, "w:line"),
        Some(288),
        "body 0.7rem after must keep heading 1.2 leading: {h1_p}"
    );
    assert!(
        !h1_p.contains("<w:sz ") && !h2_p.contains("<w:sz "),
        "body 0.7rem after must not reintroduce markdown CSS w:sz: {h1_p} {h2_p}"
    );
    let h1_run = run_containing(h1_p, ">Northwind Freight</w:t>").expect("H1 run");
    assert!(
        h1_run.contains(r#"<w:spacing w:val="-6"/>"#),
        "body 0.7rem after must keep H1 tracking −0.02em: {h1_run}"
    );
    let table_grid = style_block(&styles_xml, "TableGrid").expect("TableGrid style");
    assert!(
        table_grid.contains(r#"<w:vAlign w:val="top"/>"#)
            && table_grid.contains(r#"<w:sz w:val="23"/>"#),
        "body 0.7rem after must keep TableGrid valign top + 11.5pt: {table_grid}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
}

#[test]
fn letter_md_docx_body_color_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("6 September 2026"),
        "letter.md must keep the body date Word paints as letter teal, not UA black"
    );
    assert!(
        text.contains("> Replay is evidence. Same fonts, same bytes."),
        "letter.md must keep the blockquote"
    );
    assert!(
        text.contains("PDF/^A^") && text.contains("H~2~O"),
        "letter.md must keep PDF/^A^ and H~2~O"
    );
    assert!(
        text.contains("- [x] Receiving dock is clear"),
        "letter.md must keep tasks"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut styles_xml = String::new();
    zip.by_name("word/styles.xml")
        .expect("word/styles.xml")
        .read_to_string(&mut styles_xml)
        .unwrap();

    let body_p = paragraph_containing(&document_xml, "6 September 2026")
        .expect("body date paragraph");
    let body_run = run_containing(body_p, ">6 September 2026</w:t>").expect("body date run");
    assert!(
        body_run.contains(r#"<w:color w:val="134E4A"/>"#)
            && !body_run.contains(r#"w:val="000000""#)
            && !body_run.contains(r#"w:color="auto""#),
        "letter body run must be #134e4a like quote/th, not UA black: {body_run}"
    );
    let ops_p = paragraph_containing(&document_xml, "operations desk")
        .expect("body operations paragraph");
    assert!(
        ops_p.contains(r#"<w:color w:val="134E4A"/>"#)
            && !ops_p.contains(r#"w:val="000000""#),
        "letter body copy must stay letter teal, not UA black: {ops_p}"
    );
    let normal_style = style_block(&styles_xml, "Normal").expect("Normal style");
    assert!(
        normal_style.contains(r#"<w:color w:val="134E4A"/>"#)
            && !normal_style.contains(r#"w:val="000000""#)
            && !normal_style.contains(r#"w:color="auto""#),
        "Normal must keep letter #134e4a so style-only body is not UA black: {normal_style}"
    );

    let quote_p = paragraph_containing(&document_xml, "Replay is evidence")
        .expect("blockquote paragraph");
    assert!(
        quote_p.contains("<w:pBdr>")
            && quote_p.contains("<w:left ")
            && quote_p.contains(r#"w:color="134E4A""#)
            && p_bdr_left_sz(quote_p).unwrap_or(0) >= 24,
        "body color must keep quote 4px #134e4a left bar: {quote_p}"
    );
    let quote_style = style_block(&styles_xml, "Quote").expect("Quote style");
    assert!(
        quote_style.contains("<w:pBdr>")
            && quote_style.contains(r#"w:sz="24""#)
            && quote_style.contains(r#"w:color="134E4A""#),
        "body color must keep Quote style bar: {quote_style}"
    );

    let flow_p = paragraph_containing(&document_xml, "three hashes")
        .expect("letter super/sub paragraph");
    let a_run = run_containing(flow_p, ">A</w:t>").expect("PDF/^A^ run");
    let two_run = run_containing(flow_p, ">2</w:t>").expect("H~2~O run");
    assert!(
        a_run.contains(r#"<w:vertAlign w:val="superscript"/>"#)
            && two_run.contains(r#"<w:vertAlign w:val="subscript"/>"#),
        "body color must keep super/sub vertAlign: {a_run} {two_run}"
    );
    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains("<w:tcBorders>"),
        "body color must keep th teal fill: {hop_tc}"
    );
    let grid = tbl_borders_block(&document_xml).expect("w:tblBorders");
    assert!(
        grid.contains(r#"w:color="CBD5E1""#) && !grid.contains(r#"w:color="000000""#),
        "body color must keep letter 1px #cbd5e1 hop grid: {grid}"
    );
    let input_p = paragraph_containing(&document_xml, "Input").expect("table body cell");
    assert!(
        input_p.contains(r#"<w:color w:val="134E4A"/>"#) && !input_p.contains("<w:b/>"),
        "table body type must stay letter teal, not th bold or UA black: {input_p}"
    );
    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    assert_eq!(
        num_id(dock_p),
        Some(4),
        "body color must keep checked task numId 4: {dock_p}"
    );
    assert!(
        dock_p.contains(r#"<w:color w:val="134E4A"/>"#),
        "task item type must stay letter teal: {dock_p}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
}

#[test]
fn letter_md_docx_normal_line_like_word() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("6 September 2026"),
        "letter.md must keep the body date Word paints at Normal 1.15"
    );
    assert!(
        text.contains("> Replay is evidence. Same fonts, same bytes."),
        "letter.md must keep the blockquote"
    );
    assert!(
        text.contains("- [x] Receiving dock is clear"),
        "letter.md must keep tasks"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut styles_xml = String::new();
    zip.by_name("word/styles.xml")
        .expect("word/styles.xml")
        .read_to_string(&mut styles_xml)
        .unwrap();

    let normal_style = style_block(&styles_xml, "Normal").expect("Normal style");
    assert!(
        spacing_attr(normal_style, "w:line").unwrap_or(0) >= 276
            && normal_style.contains(r#"w:lineRule="auto""#),
        "Normal must keep Word 1.15 (w:line 276), not single 240: {normal_style}"
    );
    let body_p = paragraph_containing(&document_xml, "6 September 2026")
        .expect("body date paragraph");
    assert!(
        spacing_attr(body_p, "w:line").unwrap_or(0) >= 276,
        "letter body must keep Word 1.15 (276), got {:?}: {body_p}",
        spacing_attr(body_p, "w:line")
    );
    let body_run = run_containing(body_p, ">6 September 2026</w:t>").expect("body date run");
    assert!(
        body_run.contains(r#"<w:color w:val="134E4A"/>"#)
            && !body_run.contains(r#"w:val="000000""#),
        "body color must stay letter #134e4a: {body_run}"
    );

    let quote_p = paragraph_containing(&document_xml, "Replay is evidence")
        .expect("blockquote paragraph");
    assert!(
        ind_left_twips(quote_p).unwrap_or(0) >= 720,
        "letter quote indent must stay Word 0.5in: {quote_p}"
    );
    assert!(
        quote_p.contains("<w:pBdr>")
            && quote_p.contains("<w:left ")
            && quote_p.contains(r#"w:color="134E4A""#)
            && p_bdr_left_sz(quote_p).unwrap_or(0) >= 24,
        "quote bar must stay 4px #134e4a: {quote_p}"
    );

    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains("<w:tcBorders>"),
        "th teal fill must not regress: {hop_tc}"
    );
    let grid = tbl_borders_block(&document_xml).expect("w:tblBorders");
    assert!(
        grid.contains(r#"w:color="CBD5E1""#) && !grid.contains(r#"w:color="000000""#),
        "line 276 must keep letter 1px #cbd5e1 hop grid: {grid}"
    );
    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    assert_eq!(
        num_id(dock_p),
        Some(4),
        "checked task numId must not regress: {dock_p}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
}

#[test]
fn letter_md_docx_table_border_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("| Hop | File | Proof |"),
        "letter.md must keep the hop table Word paints with a 1px #cbd5e1 grid"
    );
    assert!(
        text.contains("6 September 2026"),
        "letter.md must keep the body date"
    );
    assert!(
        text.contains("> Replay is evidence. Same fonts, same bytes."),
        "letter.md must keep the blockquote"
    );
    assert!(
        text.contains("- [x] Receiving dock is clear"),
        "letter.md must keep tasks"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut styles_xml = String::new();
    zip.by_name("word/styles.xml")
        .expect("word/styles.xml")
        .read_to_string(&mut styles_xml)
        .unwrap();

    let grid = tbl_borders_block(&document_xml).expect("w:tblBorders");
    assert!(
        grid.contains(r#"w:color="CBD5E1""#)
            && grid.contains(r#"w:sz="6""#)
            && !grid.contains(r#"w:color="000000""#),
        "letter hop grid must be 1px #cbd5e1, not Word TableGrid black: {grid}"
    );
    let table_grid = style_block(&styles_xml, "TableGrid").expect("TableGrid style");
    assert!(
        table_grid.contains(r#"w:color="CBD5E1""#)
            && table_grid.contains(r#"w:sz="6""#)
            && !table_grid.contains(r#"w:color="000000""#),
        "TableGrid must keep letter 1px #cbd5e1: {table_grid}"
    );

    let body_p = paragraph_containing(&document_xml, "6 September 2026")
        .expect("body date paragraph");
    let body_run = run_containing(body_p, ">6 September 2026</w:t>").expect("body date run");
    assert!(
        body_run.contains(r#"<w:color w:val="134E4A"/>"#)
            && !body_run.contains(r#"w:val="000000""#),
        "slate grid must keep body #134e4a: {body_run}"
    );
    assert!(
        spacing_attr(body_p, "w:line").unwrap_or(0) >= 276,
        "slate grid must keep Word 1.15 (276): {body_p}"
    );
    let normal_style = style_block(&styles_xml, "Normal").expect("Normal style");
    assert!(
        spacing_attr(normal_style, "w:line").unwrap_or(0) >= 276
            && normal_style.contains(r#"<w:color w:val="134E4A"/>"#),
        "slate grid must keep Normal #134e4a + line 276: {normal_style}"
    );

    let quote_p = paragraph_containing(&document_xml, "Replay is evidence")
        .expect("blockquote paragraph");
    assert!(
        quote_p.contains("<w:pBdr>")
            && quote_p.contains("<w:left ")
            && quote_p.contains(r#"w:color="134E4A""#)
            && p_bdr_left_sz(quote_p).unwrap_or(0) >= 24,
        "slate grid must keep quote 4px #134e4a left bar: {quote_p}"
    );

    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains("<w:tcBorders>"),
        "slate grid must keep th teal fill: {hop_tc}"
    );
    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    assert_eq!(
        num_id(dock_p),
        Some(4),
        "slate grid must keep checked task numId 4: {dock_p}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
}

#[test]
fn letter_md_docx_table_type_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("| Hop | File | Proof |"),
        "letter.md must keep the hop table Word paints at letter 0.95rem, not body 11pt"
    );
    assert!(
        text.contains("6 September 2026"),
        "letter.md must keep the body date"
    );
    assert!(
        text.contains("`prove`"),
        "letter.md must keep the inline `prove` chip Word paints at .92em of 11pt"
    );
    assert!(
        text.contains("> Replay is evidence. Same fonts, same bytes."),
        "letter.md must keep the blockquote"
    );
    assert!(
        text.contains("- [x] Receiving dock is clear"),
        "letter.md must keep tasks"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut styles_xml = String::new();
    zip.by_name("word/styles.xml")
        .expect("word/styles.xml")
        .read_to_string(&mut styles_xml)
        .unwrap();

    let hop_p = paragraph_containing(&document_xml, "Hop").expect("Hop cell paragraph");
    let input_p = paragraph_containing(&document_xml, "Input").expect("Input cell paragraph");
    let hop_run = run_containing(hop_p, ">Hop</w:t>").expect("Hop run");
    let input_run = run_containing(input_p, ">Input</w:t>").expect("Input run");
    assert_eq!(
        style_sz(hop_run).expect("Hop w:sz"),
        23,
        "letter th must stay 0.95rem (sz 23), not body 11pt or code 10pt: {hop_run}"
    );
    assert_eq!(
        style_sz(input_run).expect("Input w:sz"),
        23,
        "letter td must stay 0.95rem (sz 23), not body 11pt or code 10pt: {input_run}"
    );
    assert!(
        hop_run.contains(r#"<w:szCs w:val="23"/>"#)
            && input_run.contains(r#"<w:szCs w:val="23"/>"#)
            && !hop_run.contains(r#"<w:sz w:val="22"/>"#)
            && !input_run.contains(r#"<w:sz w:val="22"/>"#)
            && !hop_run.contains(r#"<w:sz w:val="20"/>"#)
            && !input_run.contains(r#"<w:sz w:val="20"/>"#),
        "hop table type must not inherit body 11pt or the inline chip 10pt: {hop_run} {input_run}"
    );

    let table_grid = style_block(&styles_xml, "TableGrid").expect("TableGrid style");
    assert!(
        table_grid.contains(r#"<w:sz w:val="23"/>"#)
            && table_grid.contains(r#"<w:szCs w:val="23"/>"#),
        "TableGrid must keep letter 0.95rem so style-only cells are not body 11pt: {table_grid}"
    );

    let body_p = paragraph_containing(&document_xml, "6 September 2026")
        .expect("body date paragraph");
    let body_run = run_containing(body_p, ">6 September 2026</w:t>").expect("body date run");
    assert!(
        !body_run.contains(r#"<w:sz w:val="23"/>"#) && !body_run.contains(r#"<w:sz w:val="20"/>"#),
        "letter body must stay Word 11pt, not hop-table 0.95rem: {body_run}"
    );
    let prove_p = paragraph_containing(&document_xml, "three hashes")
        .expect("letter mark paragraph");
    let prove_run = run_containing(prove_p, ">prove</w:t>").expect("prove code run");
    assert_eq!(
        style_sz(prove_run).expect("prove w:sz"),
        20,
        "table type must keep inline `prove` at Word 10pt: {prove_run}"
    );
    let h1_p = paragraph_containing(&document_xml, r#"w:val="Heading1""#)
        .expect("Heading1 paragraph");
    let h2_p = paragraph_containing(&document_xml, r#"w:val="Heading2""#)
        .expect("Heading2 paragraph");
    assert!(
        !h1_p.contains("<w:sz ") && !h2_p.contains("<w:sz "),
        "table type must not reintroduce markdown CSS w:sz on headings: {h1_p} {h2_p}"
    );

    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    let input_tc = cell_containing(&document_xml, "Input").expect("Input body cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains("<w:tcBorders>"),
        "table type must keep th teal fill: {hop_tc}"
    );
    assert!(
        hop_tc.contains(r#"<w:vAlign w:val="top"/>"#)
            && input_tc.contains(r#"<w:vAlign w:val="top"/>"#),
        "table type must keep letter vertical-align top: {hop_tc} {input_tc}"
    );
    let quote_p = paragraph_containing(&document_xml, "Replay is evidence")
        .expect("blockquote paragraph");
    assert!(
        quote_p.contains("<w:pBdr>")
            && quote_p.contains("<w:left ")
            && quote_p.contains(r#"w:color="134E4A""#)
            && p_bdr_left_sz(quote_p).unwrap_or(0) >= 24,
        "table type must keep quote 4px #134e4a left bar: {quote_p}"
    );
    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    assert_eq!(
        num_id(dock_p),
        Some(4),
        "table type must keep checked task numId 4: {dock_p}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
}

#[test]
fn letter_md_docx_cell_valign_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("| Hop | File | Proof |")
            && text.contains("layout i32 @ 1/7200 in")
            && text.contains("PDF/^A^ · HTML · ODT"),
        "letter.md must keep the hop table Word paints at letter vertical-align top, not UA middle"
    );
    assert!(
        text.contains("6 September 2026"),
        "letter.md must keep the body date"
    );
    assert!(
        text.contains("`prove`"),
        "letter.md must keep the inline `prove` chip"
    );
    assert!(
        text.contains("> Replay is evidence. Same fonts, same bytes."),
        "letter.md must keep the blockquote"
    );
    assert!(
        text.contains("- [x] Receiving dock is clear"),
        "letter.md must keep tasks"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    assert!(
        doc.sections[0].body.iter().any(|b| match b {
            Block::Table(t) => t.rows.iter().any(|r| {
                r.cells.iter().any(|c| {
                    c.blocks.iter().any(|b| match b {
                        Block::Paragraph(p) => p.plain_text().contains("Hop"),
                        _ => false,
                    })
                })
            }),
            _ => false,
        }),
        "letter.md IR must keep the hop table"
    );

    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut styles_xml = String::new();
    zip.by_name("word/styles.xml")
        .expect("word/styles.xml")
        .read_to_string(&mut styles_xml)
        .unwrap();

    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    let file_tc = cell_containing(&document_xml, "File").expect("File header cell");
    let proof_tc = cell_containing(&document_xml, "Proof").expect("Proof header cell");
    let input_tc = cell_containing(&document_xml, "Input").expect("Input body cell");
    let plan_tc = cell_containing(&document_xml, "layout i32").expect("wrapped plan body cell");
    let sha_tc = cell_containing(&document_xml, "input SHA-256").expect("SHA body cell");
    for (name, tc) in [
        ("Hop", hop_tc),
        ("File", file_tc),
        ("Proof", proof_tc),
        ("Input", input_tc),
        ("plan", plan_tc),
        ("SHA", sha_tc),
    ] {
        assert!(
            tc.contains(r#"<w:vAlign w:val="top"/>"#),
            "letter {name} must keep letter.html th,td vertical-align top: {tc}"
        );
        assert!(
            !tc.contains(r#"w:val="center""#)
                && !tc.contains(r#"w:val="both""#)
                && !tc.contains(r#"w:val="bottom""#),
            "UA/python-docx center {name} punches sparse bands vs WeasyPrint letter input: {tc}"
        );
    }
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains("<w:tcBorders>"),
        "cell valign must keep th teal fill: {hop_tc}"
    );
    assert!(
        !input_tc.contains(r#"w:fill="CCFBF1""#) && !sha_tc.contains(r#"w:fill="CCFBF1""#),
        "cell valign must not leak th fill onto td: {input_tc} {sha_tc}"
    );

    let table_grid = style_block(&styles_xml, "TableGrid").expect("TableGrid style");
    assert!(
        table_grid.contains(r#"<w:vAlign w:val="top"/>"#)
            && !table_grid.contains(r#"w:val="center""#)
            && !table_grid.contains(r#"w:val="bottom""#),
        "TableGrid must keep letter vertical-align top so style-only cells are not middle: {table_grid}"
    );
    assert!(
        table_grid.contains(r#"w:type="firstRow""#)
            && table_grid.contains(r#"w:fill="CCFBF1""#)
            && table_grid.contains(r#"<w:sz w:val="23"/>"#),
        "cell valign must keep TableGrid firstRow fill + 0.95rem: {table_grid}"
    );

    let hop_run = run_containing(hop_tc, ">Hop</w:t>").expect("Hop run");
    let input_run = run_containing(input_tc, ">Input</w:t>").expect("Input run");
    assert_eq!(
        style_sz(hop_run).expect("Hop w:sz"),
        23,
        "cell valign must keep letter 0.95rem table type: {hop_run}"
    );
    assert_eq!(
        style_sz(input_run).expect("Input w:sz"),
        23,
        "cell valign must keep letter td 0.95rem: {input_run}"
    );

    let body_p = paragraph_containing(&document_xml, "6 September 2026")
        .expect("body date paragraph");
    let body_run = run_containing(body_p, ">6 September 2026</w:t>").expect("body date run");
    assert!(
        body_run.contains(r#"<w:color w:val="134E4A"/>"#)
            && !body_p.contains(r#"<w:vAlign "#),
        "cell valign must keep body #134e4a and not leak onto Normal: {body_run}"
    );
    assert_eq!(
        spacing_attr(body_p, "w:line"),
        Some(276),
        "cell valign must keep Word 1.15 (276): {body_p}"
    );

    let quote_p = paragraph_containing(&document_xml, "Replay is evidence")
        .expect("blockquote paragraph");
    assert!(
        quote_p.contains("<w:pBdr>")
            && quote_p.contains("<w:left ")
            && quote_p.contains(r#"w:color="134E4A""#)
            && p_bdr_left_sz(quote_p).unwrap_or(0) >= 24,
        "cell valign must keep quote 4px #134e4a left bar: {quote_p}"
    );
    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    assert_eq!(
        num_id(dock_p),
        Some(4),
        "cell valign must keep checked task numId 4: {dock_p}"
    );
    let h1_p = paragraph_containing(&document_xml, r#"w:val="Heading1""#)
        .expect("Heading1 paragraph");
    let h2_p = paragraph_containing(&document_xml, r#"w:val="Heading2""#)
        .expect("Heading2 paragraph");
    assert!(
        !h1_p.contains("<w:sz ") && !h2_p.contains("<w:sz "),
        "cell valign must not reintroduce markdown CSS w:sz on headings: {h1_p} {h2_p}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
}

#[test]
fn letter_md_docx_heading_color_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("# Northwind Freight") && text.contains("## Delivery confirmation"),
        "letter.md must keep H1/H2 Word paints as letter teal, not Calibri blue"
    );
    assert!(
        text.contains("6 September 2026"),
        "letter.md must keep the body date"
    );
    assert!(
        text.contains("> Replay is evidence. Same fonts, same bytes."),
        "letter.md must keep the blockquote"
    );
    assert!(
        text.contains("- [x] Receiving dock is clear"),
        "letter.md must keep tasks"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut styles_xml = String::new();
    zip.by_name("word/styles.xml")
        .expect("word/styles.xml")
        .read_to_string(&mut styles_xml)
        .unwrap();

    let h1_p = paragraph_containing(&document_xml, r#"w:val="Heading1""#)
        .expect("Heading1 paragraph");
    let h2_p = paragraph_containing(&document_xml, r#"w:val="Heading2""#)
        .expect("Heading2 paragraph");
    let h1_run = run_containing(h1_p, ">Northwind Freight</w:t>").expect("H1 run");
    let h2_run = run_containing(h2_p, ">Delivery confirmation").expect("H2 run");
    assert!(
        h1_run.contains(r#"<w:color w:val="134E4A"/>"#)
            && !h1_run.contains(r#"w:val="2F5496""#)
            && !h1_run.contains(r#"w:val="000000""#)
            && !h1_run.contains(r#"w:color="auto""#),
        "letter H1 run must be #134e4a like body/quote/th, not Word 2F5496: {h1_run}"
    );
    assert!(
        h2_run.contains(r#"<w:color w:val="134E4A"/>"#) && !h2_run.contains(r#"w:val="2F5496""#),
        "letter H2 run must be #134e4a, not Word Calibri blue: {h2_run}"
    );
    assert!(
        !h1_p.contains("<w:sz ") && !h2_p.contains("<w:sz "),
        "heading color must not reintroduce markdown CSS w:sz: {h1_p} {h2_p}"
    );
    let h1_style = style_block(&styles_xml, "Heading1").expect("Heading1 style");
    let h2_style = style_block(&styles_xml, "Heading2").expect("Heading2 style");
    assert!(
        h1_style.contains(r#"<w:color w:val="134E4A"/>"#)
            && h2_style.contains(r#"<w:color w:val="134E4A"/>"#)
            && !h1_style.contains(r#"w:val="2F5496""#)
            && !h2_style.contains(r#"w:val="2F5496""#)
            && !styles_xml.contains(r#"w:val="2F5496""#),
        "Heading1/2 style type must stay letter #134e4a, not Word 2F5496: {h1_style} {h2_style}"
    );

    let body_p = paragraph_containing(&document_xml, "6 September 2026")
        .expect("body date paragraph");
    let body_run = run_containing(body_p, ">6 September 2026</w:t>").expect("body date run");
    assert!(
        body_run.contains(r#"<w:color w:val="134E4A"/>"#)
            && !body_run.contains(r#"w:val="000000""#),
        "heading color must keep body #134e4a: {body_run}"
    );
    assert!(
        spacing_attr(body_p, "w:line").unwrap_or(0) >= 276,
        "heading color must keep Word 1.15 (276): {body_p}"
    );

    let quote_p = paragraph_containing(&document_xml, "Replay is evidence")
        .expect("blockquote paragraph");
    assert!(
        quote_p.contains("<w:pBdr>")
            && quote_p.contains("<w:left ")
            && quote_p.contains(r#"w:color="134E4A""#)
            && p_bdr_left_sz(quote_p).unwrap_or(0) >= 24,
        "heading color must keep quote 4px #134e4a left bar: {quote_p}"
    );

    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains("<w:tcBorders>"),
        "heading color must keep th teal fill: {hop_tc}"
    );
    let grid = tbl_borders_block(&document_xml).expect("w:tblBorders");
    assert!(
        grid.contains(r#"w:color="CBD5E1""#) && !grid.contains(r#"w:color="000000""#),
        "heading color must keep letter 1px #cbd5e1 hop grid: {grid}"
    );
    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    assert_eq!(
        num_id(dock_p),
        Some(4),
        "heading color must keep checked task numId 4: {dock_p}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
}

#[test]
fn letter_md_docx_heading_tracking_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("# Northwind Freight") && text.contains("## Delivery confirmation"),
        "letter.md must keep H1/H2 Word paints with letter.css h1 -.02em tracking"
    );
    assert!(
        text.contains("6 September 2026"),
        "letter.md must keep the body date"
    );
    assert!(
        text.contains("> Replay is evidence. Same fonts, same bytes."),
        "letter.md must keep the blockquote"
    );
    assert!(
        text.contains("- [x] Receiving dock is clear"),
        "letter.md must keep tasks"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut styles_xml = String::new();
    zip.by_name("word/styles.xml")
        .expect("word/styles.xml")
        .read_to_string(&mut styles_xml)
        .unwrap();

    let h1_p = paragraph_containing(&document_xml, r#"w:val="Heading1""#)
        .expect("Heading1 paragraph");
    let h2_p = paragraph_containing(&document_xml, r#"w:val="Heading2""#)
        .expect("Heading2 paragraph");
    let h1_run = run_containing(h1_p, ">Northwind Freight</w:t>").expect("H1 run");
    let h2_run = run_containing(h2_p, ">Delivery confirmation").expect("H2 run");
    assert!(
        h1_run.contains(r#"<w:spacing w:val="-6"/>"#)
            && !h1_run.contains(r#"<w:spacing w:val="0"/>"#),
        "letter H1 run must keep letter.css -.02em tracking (w:spacing -6 at 16pt), not 0: {h1_run}"
    );
    assert!(
        !h2_run.contains("<w:spacing ") && !h2_p.contains(r#"<w:spacing w:val="#),
        "letter.css letter-spacing is h1-only, not Heading 2: {h2_run}"
    );
    assert!(
        !h1_p.contains("<w:sz ") && !h2_p.contains("<w:sz "),
        "h1 tracking must not reintroduce markdown CSS w:sz: {h1_p} {h2_p}"
    );
    let h1_style = style_block(&styles_xml, "Heading1").expect("Heading1 style");
    let h2_style = style_block(&styles_xml, "Heading2").expect("Heading2 style");
    let h3_style = style_block(&styles_xml, "Heading3").expect("Heading3 style");
    assert!(
        h1_style.contains(r#"<w:spacing w:val="-6"/>"#)
            && !h1_style.contains(r#"<w:spacing w:val="0"/>"#),
        "Heading1 style must keep letter.css -.02em tracking: {h1_style}"
    );
    assert!(
        !h2_style.contains(r#"<w:spacing w:val="#)
            && !h3_style.contains(r#"<w:spacing w:val="#),
        "Heading2/3 must not pick up h1 tracking: {h2_style} {h3_style}"
    );
    assert!(
        h1_run.contains(r#"<w:color w:val="134E4A"/>"#)
            && h1_style.contains(r#"<w:color w:val="134E4A"/>"#)
            && !h1_run.contains(r#"w:val="2F5496""#),
        "h1 tracking must keep heading letter teal: {h1_run} {h1_style}"
    );
    assert!(
        h1_p.contains("<w:pBdr>")
            && h1_p.contains(r#"w:color="134E4A""#)
            && p_bdr_bottom_sz(h1_p).unwrap_or(0) >= 18,
        "h1 tracking must keep letter 3px #134e4a rule: {h1_p}"
    );

    let body_p = paragraph_containing(&document_xml, "6 September 2026")
        .expect("body date paragraph");
    let body_run = run_containing(body_p, ">6 September 2026</w:t>").expect("body date run");
    assert!(
        body_run.contains(r#"<w:color w:val="134E4A"/>"#)
            && !body_run.contains("<w:spacing ")
            && !body_run.contains(r#"w:val="000000""#),
        "h1 tracking must keep body #134e4a and not leak onto Normal: {body_run}"
    );
    assert!(
        spacing_attr(body_p, "w:line").unwrap_or(0) >= 276,
        "h1 tracking must keep Word 1.15 (276): {body_p}"
    );

    let quote_p = paragraph_containing(&document_xml, "Replay is evidence")
        .expect("blockquote paragraph");
    assert!(
        quote_p.contains("<w:pBdr>")
            && quote_p.contains("<w:left ")
            && quote_p.contains(r#"w:color="134E4A""#)
            && p_bdr_left_sz(quote_p).unwrap_or(0) >= 24,
        "h1 tracking must keep quote 4px #134e4a left bar: {quote_p}"
    );

    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains("<w:tcBorders>"),
        "h1 tracking must keep th teal fill: {hop_tc}"
    );
    let hop_run = run_containing(hop_tc, ">Hop</w:t>").expect("Hop run");
    assert_eq!(
        style_sz(hop_run).expect("Hop w:sz"),
        23,
        "h1 tracking must keep letter 0.95rem table type: {hop_run}"
    );
    let grid = tbl_borders_block(&document_xml).expect("w:tblBorders");
    assert!(
        grid.contains(r#"w:color="CBD5E1""#) && !grid.contains(r#"w:color="000000""#),
        "h1 tracking must keep letter 1px #cbd5e1 hop grid: {grid}"
    );
    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    assert_eq!(
        num_id(dock_p),
        Some(4),
        "h1 tracking must keep checked task numId 4: {dock_p}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
}

#[test]
fn letter_md_docx_heading_leading_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("# Northwind Freight") && text.contains("## Delivery confirmation"),
        "letter.md must keep H1/H2 Word paints at letter.css line-height:1.2"
    );
    assert!(
        text.contains("6 September 2026"),
        "letter.md must keep the body date"
    );
    assert!(
        text.contains("> Replay is evidence. Same fonts, same bytes."),
        "letter.md must keep the blockquote"
    );
    assert!(
        text.contains("- [x] Receiving dock is clear"),
        "letter.md must keep tasks"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    assert!(
        doc.sections[0].body.iter().any(|b| match b {
            Block::Paragraph(p) => {
                p.outline_level == Some(1) && p.plain_text().contains("Northwind Freight")
            }
            _ => false,
        }),
        "letter.md IR must keep the H1"
    );

    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut styles_xml = String::new();
    zip.by_name("word/styles.xml")
        .expect("word/styles.xml")
        .read_to_string(&mut styles_xml)
        .unwrap();

    let h1_p = paragraph_containing(&document_xml, r#"w:val="Heading1""#)
        .expect("Heading1 paragraph");
    let h2_p = paragraph_containing(&document_xml, r#"w:val="Heading2""#)
        .expect("Heading2 paragraph");
    assert_eq!(
        spacing_attr(h1_p, "w:line"),
        Some(288),
        "letter H1 must keep letter.css line-height:1.2 (w:line 288), not body 276: {h1_p}"
    );
    assert_eq!(
        spacing_attr(h2_p, "w:line"),
        Some(288),
        "letter H2 must keep letter.css line-height:1.2 (w:line 288), not body 276: {h2_p}"
    );
    assert!(
        h1_p.contains(r#"w:lineRule="auto""#)
            && h2_p.contains(r#"w:lineRule="auto""#)
            && !h1_p.contains(r#"w:line="276""#)
            && !h2_p.contains(r#"w:line="276""#)
            && !h1_p.contains(r#"w:line="240""#),
        "inherited Normal 1.15 / single is jammed heading leading vs WeasyPrint: {h1_p}"
    );
    let h1_run = run_containing(h1_p, ">Northwind Freight</w:t>").expect("H1 run");
    let h2_run = run_containing(h2_p, ">Delivery confirmation").expect("H2 run");
    assert!(
        h1_run.contains(r#"<w:spacing w:val="-6"/>"#),
        "h1 leading must keep letter.css -.02em tracking: {h1_run}"
    );
    assert!(
        !h2_run.contains("<w:spacing ") && !h2_p.contains(r#"<w:spacing w:val="#),
        "letter.css letter-spacing is h1-only, not Heading 2: {h2_run}"
    );
    assert!(
        !h1_p.contains("<w:sz ") && !h2_p.contains("<w:sz "),
        "h1 leading must not reintroduce markdown CSS w:sz: {h1_p} {h2_p}"
    );
    let h1_style = style_block(&styles_xml, "Heading1").expect("Heading1 style");
    let h2_style = style_block(&styles_xml, "Heading2").expect("Heading2 style");
    let h3_style = style_block(&styles_xml, "Heading3").expect("Heading3 style");
    assert_eq!(
        spacing_attr(h1_style, "w:line"),
        Some(288),
        "Heading1 style must keep letter.css line-height:1.2: {h1_style}"
    );
    assert_eq!(
        spacing_attr(h2_style, "w:line"),
        Some(288),
        "Heading2 style must keep letter.css line-height:1.2: {h2_style}"
    );
    assert_eq!(
        spacing_attr(h3_style, "w:line"),
        Some(288),
        "Heading3 style must keep letter heading 1.2 leading: {h3_style}"
    );
    assert!(
        h1_style.contains(r#"<w:spacing w:val="-6"/>"#)
            && h1_style.contains(r#"<w:color w:val="134E4A"/>"#),
        "Heading1 style must keep letter tracking + teal: {h1_style}"
    );
    assert!(
        !h2_style.contains(r#"<w:spacing w:val="#)
            && !h3_style.contains(r#"<w:spacing w:val="#),
        "Heading2/3 must not pick up h1 tracking: {h2_style} {h3_style}"
    );
    assert!(
        h1_run.contains(r#"<w:color w:val="134E4A"/>"#) && !h1_run.contains(r#"w:val="2F5496""#),
        "h1 leading must keep heading letter teal: {h1_run}"
    );
    assert!(
        h1_p.contains("<w:pBdr>")
            && h1_p.contains(r#"w:color="134E4A""#)
            && p_bdr_bottom_sz(h1_p).unwrap_or(0) >= 18,
        "h1 leading must keep letter 3px #134e4a rule: {h1_p}"
    );
    assert!(
        h2_p.contains("<w:pBdr>")
            && h2_p.contains(r#"w:color="0D9488""#)
            && p_bdr_bottom_sz(h2_p).unwrap_or(0) >= 12,
        "h1 leading must keep letter 2px #0d9488 H2 rule: {h2_p}"
    );

    let body_p = paragraph_containing(&document_xml, "6 September 2026")
        .expect("body date paragraph");
    let body_run = run_containing(body_p, ">6 September 2026</w:t>").expect("body date run");
    assert_eq!(
        spacing_attr(body_p, "w:line"),
        Some(276),
        "h1 leading must keep Word 1.15 (276), not heading 1.2: {body_p}"
    );
    assert!(
        body_run.contains(r#"<w:color w:val="134E4A"/>"#) && !body_p.contains(r#"w:line="288""#),
        "h1 leading must keep body #134e4a and not leak 1.2 onto Normal: {body_run}"
    );
    let normal_style = style_block(&styles_xml, "Normal").expect("Normal style");
    assert_eq!(
        spacing_attr(normal_style, "w:line"),
        Some(276),
        "Normal must stay Word 1.15, not heading 1.2: {normal_style}"
    );

    let quote_p = paragraph_containing(&document_xml, "Replay is evidence")
        .expect("blockquote paragraph");
    assert_eq!(
        spacing_attr(quote_p, "w:line"),
        Some(276),
        "quote must keep Word 1.15, not heading 1.2: {quote_p}"
    );
    assert!(
        quote_p.contains("<w:pBdr>")
            && quote_p.contains("<w:left ")
            && quote_p.contains(r#"w:color="134E4A""#)
            && p_bdr_left_sz(quote_p).unwrap_or(0) >= 24,
        "h1 leading must keep quote 4px #134e4a left bar: {quote_p}"
    );

    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains("<w:tcBorders>"),
        "h1 leading must keep th teal fill: {hop_tc}"
    );
    let hop_run = run_containing(hop_tc, ">Hop</w:t>").expect("Hop run");
    assert_eq!(
        style_sz(hop_run).expect("Hop w:sz"),
        23,
        "h1 leading must keep letter 0.95rem table type: {hop_run}"
    );
    let grid = tbl_borders_block(&document_xml).expect("w:tblBorders");
    assert!(
        grid.contains(r#"w:color="CBD5E1""#) && !grid.contains(r#"w:color="000000""#),
        "h1 leading must keep letter 1px #cbd5e1 hop grid: {grid}"
    );
    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    assert_eq!(
        num_id(dock_p),
        Some(4),
        "h1 leading must keep checked task numId 4: {dock_p}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
}

#[test]
fn letter_md_docx_code_block_pad_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("```") && text.contains("docagent prove examples/letter.md"),
        "letter.md must keep the fenced prove block Word paints with letter pre pad"
    );
    assert!(
        text.contains("> Replay is evidence. Same fonts, same bytes."),
        "letter.md must keep the blockquote"
    );
    assert!(
        text.contains("| Hop | File | Proof |"),
        "letter.md must keep the hop table"
    );
    assert!(
        text.contains("- [x] Receiving dock is clear"),
        "letter.md must keep tasks"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    assert!(
        doc.sections[0].body.iter().any(|b| match b {
            Block::Paragraph(p) => p.code_block && p.plain_text().contains("docagent prove"),
            _ => false,
        }),
        "letter.md IR must keep the prove code_block"
    );

    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut styles_xml = String::new();
    zip.by_name("word/styles.xml")
        .expect("word/styles.xml")
        .read_to_string(&mut styles_xml)
        .unwrap();

    let code_p = paragraph_containing(&document_xml, "docagent prove")
        .expect("code_block paragraph");
    assert!(
        code_p.contains(r#"w:val="CodeBlock""#) && code_p.contains(r#"w:fill="CCFBF1""#),
        "letter prove block must stay a teal CodeBlock wash: {code_p}"
    );
    assert_eq!(
        (ind_left_twips(code_p), ind_right_twips(code_p)),
        (Some(240), Some(240)),
        "letter pre pad must be 1rem (240 twips) left/right, not jammed on the wash: {code_p}"
    );
    assert!(
        !code_p.contains(r#"w:left="0""#) && !code_p.contains(r#"w:right="0""#),
        "zero CodeBlock indent is WeasyPrint letter pre with no pad: {code_p}"
    );
    let code_style = style_block(&styles_xml, "CodeBlock").expect("CodeBlock style");
    assert_eq!(
        (ind_left_twips(code_style), ind_right_twips(code_style)),
        (Some(240), Some(240)),
        "CodeBlock style must keep letter pre 1rem pad: {code_style}"
    );
    assert_eq!(
        spacing_before_twips(code_p),
        Some(144),
        "code pad must keep letter.css 0.6rem before: {code_p}"
    );
    assert!(
        spacing_after_twips(code_p).unwrap_or(0) >= 240,
        "code pad must keep 1rem after the prove block before the hop table: {code_p}"
    );
    assert!(
        code_p.contains("<w:pBdr>")
            && p_bdr_top_sz(code_p).unwrap_or(0) >= 72
            && p_bdr_bottom_sz(code_p).unwrap_or(0) >= 72
            && code_p.contains(r#"w:color="CCFBF1""#),
        "letter pre must keep 0.75rem same-fill Y pad, not a flush wash: {code_p}"
    );
    assert!(
        code_style.contains("<w:pBdr>")
            && p_bdr_top_sz(code_style).unwrap_or(0) >= 72
            && p_bdr_bottom_sz(code_style).unwrap_or(0) >= 72
            && code_style.contains(r#"w:color="CCFBF1""#),
        "CodeBlock style must keep letter pre 0.75rem Y pad: {code_style}"
    );

    let quote_p = paragraph_containing(&document_xml, "Replay is evidence")
        .expect("blockquote paragraph");
    assert!(
        quote_p.contains("<w:pBdr>")
            && quote_p.contains("<w:left ")
            && quote_p.contains(r#"w:color="134E4A""#)
            && p_bdr_left_sz(quote_p).unwrap_or(0) >= 24,
        "code pad must keep quote 4px #134e4a left bar: {quote_p}"
    );
    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains("<w:tcBorders>"),
        "code pad must keep th teal fill: {hop_tc}"
    );
    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    assert_eq!(
        num_id(dock_p),
        Some(4),
        "code pad must keep checked task numId 4: {dock_p}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
}

#[test]
fn letter_md_docx_code_block_pad_y_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("```") && text.contains("docagent prove examples/letter.md"),
        "letter.md must keep the fenced prove block Word paints with letter pre Y pad"
    );
    assert!(
        text.contains("`prove`"),
        "letter.md must keep the inline `prove` chip Word paints at .92em of 11pt"
    );
    assert!(
        text.contains("> Replay is evidence. Same fonts, same bytes."),
        "letter.md must keep the blockquote"
    );
    assert!(
        text.contains("| Hop | File | Proof |"),
        "letter.md must keep the hop table"
    );
    assert!(
        text.contains("- [x] Receiving dock is clear"),
        "letter.md must keep tasks"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    assert!(
        doc.sections[0].body.iter().any(|b| match b {
            Block::Paragraph(p) => p.code_block && p.plain_text().contains("docagent prove"),
            _ => false,
        }),
        "letter.md IR must keep the prove code_block"
    );

    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut styles_xml = String::new();
    zip.by_name("word/styles.xml")
        .expect("word/styles.xml")
        .read_to_string(&mut styles_xml)
        .unwrap();

    let code_p = paragraph_containing(&document_xml, "docagent prove")
        .expect("code_block paragraph");
    assert!(
        code_p.contains(r#"w:val="CodeBlock""#) && code_p.contains(r#"w:fill="CCFBF1""#),
        "letter prove block must stay a teal CodeBlock wash: {code_p}"
    );
    assert_eq!(
        (ind_left_twips(code_p), ind_right_twips(code_p)),
        (Some(240), Some(240)),
        "code Y pad must keep letter pre 1rem X pad: {code_p}"
    );
    assert!(
        code_p.contains("<w:pBdr>") && code_p.contains("<w:top ") && code_p.contains("<w:bottom "),
        "Word paints pre Y pad as paragraph top/bottom borders: {code_p}"
    );
    assert_eq!(
        (p_bdr_top_sz(code_p), p_bdr_bottom_sz(code_p)),
        (Some(72), Some(72)),
        "letter pre Y pad must be 0.75rem (sz 72), not a flush wash: {code_p}"
    );
    assert!(
        code_p.contains(r#"w:color="CCFBF1""#)
            && !code_p.contains(r#"w:sz="6""#)
            && !code_p.contains(r#"w:sz="8""#),
        "code Y pad must stay the teal wash, not a hairline box: {code_p}"
    );
    assert!(
        !code_p.contains(r#"<w:sz w:val="20"/>"#),
        "letter pre inherits Word 11pt, not the inline .92em chip: {code_p}"
    );
    let code_style = style_block(&styles_xml, "CodeBlock").expect("CodeBlock style");
    assert_eq!(
        (p_bdr_top_sz(code_style), p_bdr_bottom_sz(code_style)),
        (Some(72), Some(72)),
        "CodeBlock style must keep letter pre 0.75rem Y pad: {code_style}"
    );
    assert!(
        code_style.contains(r#"w:color="CCFBF1""#) && code_style.contains(r#"w:sz="72""#),
        "CodeBlock style Y pad must stay 9pt #ccfbf1: {code_style}"
    );
    assert_eq!(
        (ind_left_twips(code_style), ind_right_twips(code_style)),
        (Some(240), Some(240)),
        "code Y pad must keep CodeBlock 1rem X pad: {code_style}"
    );

    let prove_p = paragraph_containing(&document_xml, "three hashes")
        .expect("letter mark paragraph");
    let prove_run = run_containing(prove_p, ">prove</w:t>").expect("prove code run");
    assert_eq!(
        style_sz(prove_run).expect("prove w:sz"),
        20,
        "code Y pad must keep inline `prove` at Word 10pt: {prove_run}"
    );

    let quote_p = paragraph_containing(&document_xml, "Replay is evidence")
        .expect("blockquote paragraph");
    assert!(
        quote_p.contains("<w:pBdr>")
            && quote_p.contains("<w:left ")
            && quote_p.contains(r#"w:color="134E4A""#)
            && p_bdr_left_sz(quote_p).unwrap_or(0) >= 24,
        "code Y pad must keep quote 4px #134e4a left bar: {quote_p}"
    );
    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains("<w:tcBorders>"),
        "code Y pad must keep th teal fill: {hop_tc}"
    );
    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    assert_eq!(
        num_id(dock_p),
        Some(4),
        "code Y pad must keep checked task numId 4: {dock_p}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
}

#[test]
fn letter_md_docx_task_box_size_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("- [x] Receiving dock is clear")
            && text.contains("- [ ] Replay the convert instead of hoping"),
        "letter.md must keep the task items Word paints as 12pt ☐/☒"
    );
    assert!(
        text.contains("> Replay is evidence. Same fonts, same bytes."),
        "letter.md must keep the blockquote"
    );
    assert!(
        text.contains("| Hop | File | Proof |"),
        "letter.md must keep the hop table"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    assert!(
        doc.sections[0].body.iter().any(|b| match b {
            Block::Paragraph(p) => {
                p.numbering
                    .as_ref()
                    .is_some_and(|n| n.format == Some(NumberFormat::Task))
                    && p.plain_text().contains("Receiving dock is clear")
            }
            _ => false,
        }),
        "letter.md IR must keep the checked task"
    );

    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut numbering_xml = String::new();
    zip.by_name("word/numbering.xml")
        .expect("word/numbering.xml")
        .read_to_string(&mut numbering_xml)
        .unwrap();

    assert!(
        numbering_xml.contains('\u{2610}') && numbering_xml.contains('\u{2612}'),
        "letter task numbering must stay Word ☐/☒: {numbering_xml}"
    );
    assert!(
        numbering_xml.contains("Segoe UI Symbol") && numbering_xml.contains("MS Gothic"),
        "letter task checkboxes must keep Word checkbox fonts: {numbering_xml}"
    );
    assert!(
        numbering_xml.contains(r#"<w:sz w:val="24"/>"#)
            && numbering_xml.contains(r#"<w:szCs w:val="24"/>"#)
            && numbering_xml.contains(r#"<w:color w:val="134E4A"/>"#),
        "letter task box must be 12pt #134e4a (.9em + 1.5pt border), not a missing sz: {numbering_xml}"
    );
    assert!(
        !numbering_xml.contains(r#"<w:sz w:val="22"/>"#)
            && !numbering_xml.contains(r#"<w:sz w:val="16"/>"#)
            && !numbering_xml.contains(r#"<w:sz w:val="20"/>"#),
        "letter task box must not inherit body 11pt or code 10pt: {numbering_xml}"
    );
    assert!(
        !numbering_xml.contains("[x]") && !numbering_xml.contains("[ ]"),
        "letter numbering must not use ASCII task boxes: {numbering_xml}"
    );

    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    let last_task_p = paragraph_containing(&document_xml, "Replay the convert instead of hoping")
        .expect("unchecked task");
    assert_eq!(
        num_id(dock_p),
        Some(4),
        "task box size must keep checked numId 4: {dock_p}"
    );
    assert_eq!(
        num_id(last_task_p),
        Some(3),
        "task box size must keep unchecked numId 3: {last_task_p}"
    );

    let quote_p = paragraph_containing(&document_xml, "Replay is evidence")
        .expect("blockquote paragraph");
    assert!(
        quote_p.contains("<w:pBdr>")
            && quote_p.contains("<w:left ")
            && quote_p.contains(r#"w:color="134E4A""#)
            && p_bdr_left_sz(quote_p).unwrap_or(0) >= 24,
        "task box size must keep quote 4px #134e4a left bar: {quote_p}"
    );
    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains("<w:tcBorders>"),
        "task box size must keep th teal fill: {hop_tc}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
}

#[test]
fn letter_md_docx_body_font_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("6 September 2026"),
        "letter.md must keep the body date Word paints as letter Segoe UI, not theme Calibri"
    );
    assert!(
        text.contains("# Northwind Freight") && text.contains("## Delivery confirmation"),
        "letter.md must keep H1/H2"
    );
    assert!(
        text.contains("> Replay is evidence. Same fonts, same bytes."),
        "letter.md must keep the blockquote"
    );
    assert!(
        text.contains("```") && text.contains("docagent prove examples/letter.md"),
        "letter.md must keep the prove code_block Word paints as Consolas"
    );
    assert!(
        text.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
        "letter.md must keep the DocAgent hyperlink"
    );
    assert!(
        text.contains("- [x] Receiving dock is clear"),
        "letter.md must keep tasks"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut styles_xml = String::new();
    zip.by_name("word/styles.xml")
        .expect("word/styles.xml")
        .read_to_string(&mut styles_xml)
        .unwrap();

    let body_p = paragraph_containing(&document_xml, "6 September 2026")
        .expect("body date paragraph");
    let body_run = run_containing(body_p, ">6 September 2026</w:t>").expect("body date run");
    assert!(
        body_run.contains(r#"w:ascii="Segoe UI""#)
            && body_run.contains(r#"w:hAnsi="Segoe UI""#)
            && !body_run.contains("Calibri")
            && !body_run.contains(r#"w:ascii="Times New Roman""#),
        "letter body run must be Segoe UI like letter.html, not Word Calibri: {body_run}"
    );
    let h1_p = paragraph_containing(&document_xml, r#"w:val="Heading1""#)
        .expect("Heading1 paragraph");
    let h2_p = paragraph_containing(&document_xml, r#"w:val="Heading2""#)
        .expect("Heading2 paragraph");
    let h1_run = run_containing(h1_p, ">Northwind Freight</w:t>").expect("H1 run");
    let h2_run = run_containing(h2_p, ">Delivery confirmation").expect("H2 run");
    assert!(
        h1_run.contains(r#"w:ascii="Segoe UI""#)
            && h2_run.contains(r#"w:ascii="Segoe UI""#)
            && !h1_run.contains("Calibri")
            && !h2_run.contains("Calibri"),
        "letter H1/H2 runs must stay Segoe UI, not theme Calibri: {h1_run} {h2_run}"
    );
    let normal_style = style_block(&styles_xml, "Normal").expect("Normal style");
    let h1_style = style_block(&styles_xml, "Heading1").expect("Heading1 style");
    let h2_style = style_block(&styles_xml, "Heading2").expect("Heading2 style");
    let h3_style = style_block(&styles_xml, "Heading3").expect("Heading3 style");
    let quote_style = style_block(&styles_xml, "Quote").expect("Quote style");
    let list_style = style_block(&styles_xml, "ListParagraph").expect("ListParagraph style");
    let link_style = style_block(&styles_xml, "Hyperlink").expect("Hyperlink style");
    for (name, style) in [
        ("Normal", normal_style),
        ("Heading1", h1_style),
        ("Heading2", h2_style),
        ("Heading3", h3_style),
        ("Quote", quote_style),
        ("ListParagraph", list_style),
        ("Hyperlink", link_style),
    ] {
        assert!(
            style.contains(r#"w:ascii="Segoe UI""#)
                && style.contains(r#"w:hAnsi="Segoe UI""#)
                && !style.contains("Calibri"),
            "{name} must keep letter Segoe UI so style-only type is not Calibri: {style}"
        );
    }
    assert!(
        !h1_p.contains("<w:sz ") && !h2_p.contains("<w:sz "),
        "body font must not reintroduce markdown CSS w:sz: {h1_p} {h2_p}"
    );

    let quote_p = paragraph_containing(&document_xml, "Replay is evidence")
        .expect("blockquote paragraph");
    assert!(
        quote_p.contains(r#"w:ascii="Segoe UI""#)
            && quote_p.contains("<w:pBdr>")
            && quote_p.contains(r#"w:color="134E4A""#)
            && p_bdr_left_sz(quote_p).unwrap_or(0) >= 24,
        "body font must keep quote 4px #134e4a left bar: {quote_p}"
    );
    let code_p = paragraph_containing(&document_xml, "docagent prove")
        .expect("code_block paragraph");
    assert!(
        code_p.contains("Consolas") && !code_p.contains(r#"w:ascii="Segoe UI""#),
        "letter prove block must stay Consolas, not body Segoe UI: {code_p}"
    );
    let hop_p = paragraph_containing(&document_xml, "Hop").expect("Hop cell paragraph");
    assert!(
        hop_p.contains(r#"w:ascii="Segoe UI""#) && hop_p.contains("<w:b/>"),
        "letter th type must stay Segoe UI bold: {hop_p}"
    );
    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains("<w:tcBorders>"),
        "body font must keep th teal fill: {hop_tc}"
    );
    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    assert!(
        dock_p.contains(r#"w:ascii="Segoe UI""#),
        "task item type must stay letter Segoe UI: {dock_p}"
    );
    assert_eq!(
        num_id(dock_p),
        Some(4),
        "body font must keep checked task numId 4: {dock_p}"
    );
    let link_p = paragraph_containing(&document_xml, "DocAgent").expect("DocAgent link paragraph");
    assert!(
        link_p.contains(r#"w:ascii="Segoe UI""#)
            && link_p.contains(r#"w:val="0D9488""#)
            && link_p.contains("<w:hyperlink "),
        "letter hyperlink must stay Segoe UI teal: {link_p}"
    );
    let body_run_color = run_containing(body_p, ">6 September 2026</w:t>").expect("body date run");
    assert!(
        body_run_color.contains(r#"<w:color w:val="134E4A"/>"#)
            && !body_run_color.contains(r#"w:val="000000""#),
        "body font must keep letter #134e4a: {body_run_color}"
    );
    assert!(
        spacing_attr(body_p, "w:line").unwrap_or(0) >= 276,
        "body font must keep Word 1.15 (276): {body_p}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
}

#[test]
fn letter_md_docx_code_span_size_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("`prove`"),
        "letter.md must keep the inline `prove` chip Word paints at .92em of 11pt"
    );
    assert!(
        text.contains("```") && text.contains("docagent prove examples/letter.md"),
        "letter.md must keep the prove code_block Word paints as Consolas"
    );
    assert!(
        text.contains("> Replay is evidence. Same fonts, same bytes."),
        "letter.md must keep the blockquote"
    );
    assert!(
        text.contains("| Hop | File | Proof |"),
        "letter.md must keep the hop table"
    );
    assert!(
        text.contains("- [x] Receiving dock is clear"),
        "letter.md must keep tasks"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    assert!(
        doc.sections[0].body.iter().any(|b| match b {
            Block::Paragraph(p) => p.runs.iter().any(|r| {
                r.style.code && matches!(&r.content, RunContent::Text(t) if t == "prove")
            }),
            _ => false,
        }),
        "letter.md IR must keep inline `prove` as a code span"
    );

    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut styles_xml = String::new();
    zip.by_name("word/styles.xml")
        .expect("word/styles.xml")
        .read_to_string(&mut styles_xml)
        .unwrap();
    let mut numbering_xml = String::new();
    zip.by_name("word/numbering.xml")
        .expect("word/numbering.xml")
        .read_to_string(&mut numbering_xml)
        .unwrap();

    let flow_p = paragraph_containing(&document_xml, "three hashes")
        .expect("letter mark paragraph");
    let prove_run = run_containing(flow_p, ">prove</w:t>").expect("prove code run");
    assert!(
        prove_run.contains(r#"w:val="Code""#)
            && prove_run.contains("Consolas")
            && prove_run.contains(r#"w:val="134E4A""#)
            && prove_run.contains(r#"w:fill="CCFBF1""#),
        "letter inline `prove` must stay a teal Consolas chip: {prove_run}"
    );
    assert_eq!(
        style_sz(prove_run).expect("prove w:sz"),
        20,
        "letter inline `prove` must stay Word 10pt (.92em of 11pt) even if Code is ignored: {prove_run}"
    );
    assert!(
        prove_run.contains(r#"<w:szCs w:val="20"/>"#)
            && !prove_run.contains(r#"<w:sz w:val="22"/>"#)
            && !prove_run.contains(r#"<w:sz w:val="16"/>"#)
            && !prove_run.contains(r#"<w:sz w:val="24"/>"#),
        "letter inline code must not inherit body 11pt, heading, or task-box sz: {prove_run}"
    );
    let code_char = style_block(&styles_xml, "Code").expect("Code character style");
    assert_eq!(
        style_sz(code_char).expect("Code w:sz"),
        20,
        "Code style must stay Word 10pt (.92em of 11pt): {code_char}"
    );

    let h1_p = paragraph_containing(&document_xml, r#"w:val="Heading1""#)
        .expect("Heading1 paragraph");
    let h2_p = paragraph_containing(&document_xml, r#"w:val="Heading2""#)
        .expect("Heading2 paragraph");
    assert!(
        !h1_p.contains("<w:sz ") && !h2_p.contains("<w:sz "),
        "code span sz must not reintroduce markdown CSS w:sz on headings: {h1_p} {h2_p}"
    );

    let code_p = paragraph_containing(&document_xml, "docagent prove")
        .expect("code_block paragraph");
    assert!(
        code_p.contains("Consolas") && !code_p.contains(r#"w:ascii="Segoe UI""#),
        "code span size must keep the prove block Consolas: {code_p}"
    );
    assert!(
        !code_p.contains(r#"<w:sz w:val="20"/>"#),
        "letter pre inherits Word 11pt, not the inline .92em chip: {code_p}"
    );

    let quote_p = paragraph_containing(&document_xml, "Replay is evidence")
        .expect("blockquote paragraph");
    assert!(
        quote_p.contains("<w:pBdr>")
            && quote_p.contains("<w:left ")
            && quote_p.contains(r#"w:color="134E4A""#)
            && p_bdr_left_sz(quote_p).unwrap_or(0) >= 24,
        "code span size must keep quote 4px #134e4a left bar: {quote_p}"
    );
    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains("<w:tcBorders>"),
        "code span size must keep th teal fill: {hop_tc}"
    );
    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    assert_eq!(
        num_id(dock_p),
        Some(4),
        "code span size must keep checked task numId 4: {dock_p}"
    );
    assert!(
        numbering_xml.contains(r#"<w:sz w:val="24"/>"#)
            && !numbering_xml.contains(r#"<w:sz w:val="20"/>"#),
        "task checkboxes must stay 12pt, not inherit the code chip 10pt: {numbering_xml}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
}

#[test]
fn letter_md_docx_list_indent_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("- [x] Receiving dock is clear")
            && text.contains("Bay 4")
            && text.contains("1. Hash the input"),
        "letter.md must keep tasks, nested Bay 4, and the decimal list Word paints at 1.25rem"
    );
    assert!(
        text.contains("> Replay is evidence. Same fonts, same bytes."),
        "letter.md must keep the blockquote Word paints at Quote 0.5in, not list 1.25rem"
    );
    assert!(
        text.contains("| Hop | File | Proof |"),
        "letter.md must keep the hop table"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    assert!(
        doc.sections[0].body.iter().any(|b| match b {
            Block::Paragraph(p) => {
                p.numbering.is_some() && p.plain_text().contains("Receiving dock is clear")
            }
            _ => false,
        }),
        "letter.md IR must keep the dock task"
    );

    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut styles_xml = String::new();
    zip.by_name("word/styles.xml")
        .expect("word/styles.xml")
        .read_to_string(&mut styles_xml)
        .unwrap();
    let mut numbering_xml = String::new();
    zip.by_name("word/numbering.xml")
        .expect("word/numbering.xml")
        .read_to_string(&mut numbering_xml)
        .unwrap();

    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    let bay_p = paragraph_containing(&document_xml, "Bay 4").expect("nested list item");
    let last_num_p = paragraph_containing(&document_xml, "Compare the capsule")
        .expect("last decimal list item");
    assert_eq!(
        (ind_left_twips(dock_p), ind_hanging_twips(dock_p)),
        (Some(300), Some(240)),
        "letter level-0 list indent must be 1.25rem/1rem (300/240 twips), not Word 0.5in/0.25in: {dock_p}"
    );
    assert_eq!(
        (ind_left_twips(bay_p), ind_hanging_twips(bay_p)),
        (Some(600), Some(240)),
        "letter nested list indent must be 2.5rem hanging (600/240), not Word 1.0in: {bay_p}"
    );
    assert_eq!(
        (ind_left_twips(last_num_p), ind_hanging_twips(last_num_p)),
        (Some(300), Some(240)),
        "letter decimal list indent must stay 1.25rem/1rem: {last_num_p}"
    );
    assert_eq!(
        num_id(dock_p),
        Some(4),
        "list indent must keep checked task numId 4: {dock_p}"
    );
    assert_eq!(
        num_id(bay_p),
        Some(1),
        "nested Bay 4 under a task must stay a disc bullet: {bay_p}"
    );
    assert_eq!(
        num_id(last_num_p),
        Some(2),
        "letter decimal list must stay Word numbering: {last_num_p}"
    );

    let list_style = style_block(&styles_xml, "ListParagraph").expect("ListParagraph style");
    assert_eq!(
        ind_left_twips(list_style),
        Some(300),
        "ListParagraph style must keep letter 1.25rem, not Word 0.5in: {list_style}"
    );
    assert_eq!(
        spacing_after_twips(list_style),
        Some(84),
        "list indent must keep letter 0.35rem after, not Word contextualSpacing 0: {list_style}"
    );
    assert!(
        !list_style.contains("<w:contextualSpacing"),
        "Word contextualSpacing jams consecutive li vs WeasyPrint 0.35rem: {list_style}"
    );
    let quote_p = paragraph_containing(&document_xml, "Replay is evidence")
        .expect("blockquote paragraph");
    assert!(
        ind_left_twips(quote_p).unwrap_or(0) >= 720,
        "list indent must not shrink Quote 0.5in to list 1.25rem: {quote_p}"
    );
    assert!(
        quote_p.contains("<w:pBdr>")
            && quote_p.contains("<w:left ")
            && quote_p.contains(r#"w:color="134E4A""#)
            && p_bdr_left_sz(quote_p).unwrap_or(0) >= 24,
        "list indent must keep quote 4px #134e4a left bar: {quote_p}"
    );

    assert!(
        numbering_xml.contains(r#"w:left="300" w:hanging="240""#)
            && numbering_xml.contains(r#"w:left="600" w:hanging="240""#)
            && numbering_xml.contains(r#"w:left="900" w:hanging="240""#),
        "numbering.xml must keep 1.25rem / 2.5rem / 3.75rem hanging: {numbering_xml}"
    );
    assert!(
        !numbering_xml.contains(r#"w:left="720" w:hanging="360""#)
            && !numbering_xml.contains(r#"w:left="1440" w:hanging="360""#)
            && !numbering_xml.contains(r#"w:left="2160" w:hanging="360""#),
        "Word 0.5in/1.0in/1.5in hanging is a sparse gutter vs WeasyPrint 1.25rem: {numbering_xml}"
    );
    assert!(
        numbering_xml.contains('\u{2610}')
            && numbering_xml.contains('\u{2612}')
            && numbering_xml.contains(r#"<w:sz w:val="24"/>"#),
        "list indent must keep 12pt ☐/☒ task boxes: {numbering_xml}"
    );

    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains("<w:tcBorders>"),
        "list indent must keep th teal fill: {hop_tc}"
    );
    let h1_p = paragraph_containing(&document_xml, r#"w:val="Heading1""#)
        .expect("Heading1 paragraph");
    let h2_p = paragraph_containing(&document_xml, r#"w:val="Heading2""#)
        .expect("Heading2 paragraph");
    assert_eq!(
        spacing_attr(h1_p, "w:line"),
        Some(288),
        "list indent must keep heading 1.2 leading: {h1_p}"
    );
    assert!(
        !h1_p.contains("<w:sz ") && !h2_p.contains("<w:sz "),
        "list indent must not reintroduce markdown CSS w:sz: {h1_p} {h2_p}"
    );
    let body_p = paragraph_containing(&document_xml, "6 September 2026")
        .expect("body date paragraph");
    let body_run = run_containing(body_p, ">6 September 2026</w:t>").expect("body date run");
    assert!(
        body_run.contains(r#"w:ascii="Segoe UI""#)
            && body_run.contains(r#"<w:color w:val="134E4A"/>"#),
        "list indent must keep Segoe UI body #134e4a: {body_run}"
    );
    assert_eq!(
        spacing_attr(body_p, "w:line"),
        Some(276),
        "list indent must keep Word 1.15 (276): {body_p}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
}

#[test]
fn letter_md_docx_list_item_gap_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("- [x] Receiving dock is clear")
            && text.contains("Bay 4")
            && text.contains("- [ ] Replay the convert instead of hoping")
            && text.contains("1. Hash the input"),
        "letter.md must keep tasks, nested Bay 4, and the decimal list Word paints at letter 0.35rem li gap"
    );
    assert!(
        text.contains("> Replay is evidence. Same fonts, same bytes."),
        "letter.md must keep the blockquote"
    );
    assert!(
        text.contains("| Hop | File | Proof |"),
        "letter.md must keep the hop table"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    assert!(
        doc.sections[0].body.iter().any(|b| match b {
            Block::Paragraph(p) => {
                p.numbering.is_some() && p.plain_text().contains("Receiving dock is clear")
            }
            _ => false,
        }),
        "letter.md IR must keep the dock task"
    );

    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut styles_xml = String::new();
    zip.by_name("word/styles.xml")
        .expect("word/styles.xml")
        .read_to_string(&mut styles_xml)
        .unwrap();

    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    let bay_p = paragraph_containing(&document_xml, "Bay 4").expect("nested list item");
    let capsule_p = paragraph_containing(&document_xml, "Capsule hashes travel with the files")
        .expect("middle task");
    let last_task_p = paragraph_containing(&document_xml, "Replay the convert instead of hoping")
        .expect("last task list item");
    let hash_p = paragraph_containing(&document_xml, "Hash the input")
        .expect("first decimal list item");
    let last_num_p = paragraph_containing(&document_xml, "Compare the capsule")
        .expect("last decimal list item");
    assert_eq!(
        spacing_after_twips(dock_p),
        Some(60),
        "letter parent of nested Bay 4 must be 0.25rem (60 twips), not sibling 0.35rem: {dock_p}"
    );
    assert_eq!(
        spacing_after_twips(bay_p),
        Some(84),
        "letter nested li must keep 0.35rem, not jammed 0 or body 8pt: {bay_p}"
    );
    assert_eq!(
        spacing_after_twips(capsule_p),
        Some(84),
        "letter middle task must keep 0.35rem between siblings: {capsule_p}"
    );
    assert_eq!(
        spacing_after_twips(hash_p),
        Some(84),
        "letter decimal li must keep 0.35rem, not jammed 0: {hash_p}"
    );
    assert_eq!(
        spacing_after_twips(last_task_p),
        Some(240),
        "last task item must keep letter 1rem, not Word 8pt or 0.35rem: {last_task_p}"
    );
    assert_eq!(
        spacing_after_twips(last_num_p),
        Some(240),
        "last decimal item must keep letter 1rem, not Word 8pt: {last_num_p}"
    );
    assert_eq!(
        (ind_left_twips(dock_p), ind_hanging_twips(dock_p)),
        (Some(300), Some(240)),
        "list item gap must keep 1.25rem/1rem hanging: {dock_p}"
    );
    assert_eq!(
        (ind_left_twips(bay_p), ind_hanging_twips(bay_p)),
        (Some(600), Some(240)),
        "list item gap must keep nested 2.5rem hanging: {bay_p}"
    );
    assert_eq!(
        num_id(dock_p),
        Some(4),
        "list item gap must keep checked task numId 4: {dock_p}"
    );
    assert_eq!(
        num_id(last_task_p),
        Some(3),
        "list item gap must keep unchecked task numId 3: {last_task_p}"
    );
    assert_eq!(
        num_id(last_num_p),
        Some(2),
        "list item gap must keep decimal numbering: {last_num_p}"
    );

    let list_style = style_block(&styles_xml, "ListParagraph").expect("ListParagraph style");
    assert_eq!(
        spacing_after_twips(list_style),
        Some(84),
        "ListParagraph style must keep letter 0.35rem so style-only li is not jammed: {list_style}"
    );
    assert!(
        !list_style.contains("<w:contextualSpacing") && !styles_xml.contains("<w:contextualSpacing"),
        "Word contextualSpacing swallows 0.35rem between consecutive ListParagraphs: {list_style}"
    );
    assert_eq!(
        ind_left_twips(list_style),
        Some(300),
        "list item gap must keep ListParagraph 1.25rem indent: {list_style}"
    );

    let quote_p = paragraph_containing(&document_xml, "Replay is evidence")
        .expect("blockquote paragraph");
    assert!(
        ind_left_twips(quote_p).unwrap_or(0) >= 720,
        "list item gap must not shrink Quote 0.5in: {quote_p}"
    );
    assert!(
        quote_p.contains("<w:pBdr>")
            && quote_p.contains("<w:left ")
            && quote_p.contains(r#"w:color="134E4A""#)
            && p_bdr_left_sz(quote_p).unwrap_or(0) >= 24,
        "list item gap must keep quote 4px #134e4a left bar: {quote_p}"
    );
    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains("<w:tcBorders>"),
        "list item gap must keep th teal fill: {hop_tc}"
    );
    let h1_p = paragraph_containing(&document_xml, r#"w:val="Heading1""#)
        .expect("Heading1 paragraph");
    let h2_p = paragraph_containing(&document_xml, r#"w:val="Heading2""#)
        .expect("Heading2 paragraph");
    assert_eq!(
        spacing_attr(h1_p, "w:line"),
        Some(288),
        "list item gap must keep heading 1.2 leading: {h1_p}"
    );
    assert!(
        !h1_p.contains("<w:sz ") && !h2_p.contains("<w:sz "),
        "list item gap must not reintroduce markdown CSS w:sz: {h1_p} {h2_p}"
    );
    let body_p = paragraph_containing(&document_xml, "6 September 2026")
        .expect("body date paragraph");
    let body_run = run_containing(body_p, ">6 September 2026</w:t>").expect("body date run");
    assert!(
        body_run.contains(r#"w:ascii="Segoe UI""#)
            && body_run.contains(r#"<w:color w:val="134E4A"/>"#),
        "list item gap must keep Segoe UI body #134e4a: {body_run}"
    );
    assert_eq!(
        spacing_attr(body_p, "w:line"),
        Some(276),
        "list item gap must keep Word 1.15 (276): {body_p}"
    );
    assert_eq!(
        spacing_after_twips(body_p),
        Some(168),
        "list item gap must keep body letter.css 0.7rem: {body_p}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
}

#[test]
fn letter_md_docx_list_block_after_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("- [x] Receiving dock is clear")
            && text.contains("- [ ] Replay the convert instead of hoping")
            && text.contains("1. Hash the input")
            && text.contains("3. Compare the capsule"),
        "letter.md must keep tasks and the decimal list Word paints with letter ul,ol 1rem after"
    );
    assert!(
        text.contains("Agent replay:"),
        "letter.md must keep the body after the task list"
    );
    assert!(
        text.contains("> Replay is evidence. Same fonts, same bytes."),
        "letter.md must keep the blockquote"
    );
    assert!(
        text.contains("| Hop | File | Proof |"),
        "letter.md must keep the hop table"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    assert!(
        doc.sections[0].body.iter().any(|b| match b {
            Block::Paragraph(p) => {
                p.numbering.is_some() && p.plain_text().contains("Replay the convert instead of hoping")
            }
            _ => false,
        }),
        "letter.md IR must keep the last task item"
    );

    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut styles_xml = String::new();
    zip.by_name("word/styles.xml")
        .expect("word/styles.xml")
        .read_to_string(&mut styles_xml)
        .unwrap();

    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    let bay_p = paragraph_containing(&document_xml, "Bay 4").expect("nested list item");
    let last_task_p = paragraph_containing(&document_xml, "Replay the convert instead of hoping")
        .expect("last task list item");
    let hash_p = paragraph_containing(&document_xml, "Hash the input")
        .expect("first decimal list item");
    let last_num_p = paragraph_containing(&document_xml, "Compare the capsule")
        .expect("last decimal list item");
    let agent_p = paragraph_containing(&document_xml, "Agent replay:")
        .expect("body after the task list");
    assert_eq!(
        spacing_after_twips(dock_p),
        Some(60),
        "parent of nested Bay 4 must stay letter 0.25rem, not leak 0.35rem/1rem: {dock_p}"
    );
    assert_eq!(
        spacing_after_twips(bay_p),
        Some(84),
        "nested li must stay letter 0.35rem: {bay_p}"
    );
    assert_eq!(
        spacing_after_twips(hash_p),
        Some(84),
        "consecutive decimal li must stay letter 0.35rem: {hash_p}"
    );
    assert_eq!(
        spacing_after_twips(last_task_p),
        Some(240),
        "last task must keep letter.html ul,ol 1rem (240 twips), not Word 8pt: {last_task_p}"
    );
    assert_eq!(
        spacing_after_twips(last_num_p),
        Some(240),
        "last decimal must keep letter.html ul,ol 1rem (240 twips), not Word 8pt: {last_num_p}"
    );
    assert!(
        !last_task_p.contains(r#"w:after="160""#)
            && !last_num_p.contains(r#"w:after="160""#)
            && !last_task_p.contains(r#"w:after="84""#)
            && !last_num_p.contains(r#"w:after="84""#),
        "Word Normal 8pt / consecutive 0.35rem last-item is jammed vs WeasyPrint 1rem: {last_task_p} {last_num_p}"
    );
    assert!(
        spacing_after_twips(agent_p).unwrap_or(0) >= 160
            && spacing_after_twips(agent_p).unwrap_or(0) < 240,
        "list 1rem after must not leak onto the next body: {agent_p}"
    );
    assert_eq!(
        (ind_left_twips(dock_p), ind_hanging_twips(dock_p)),
        (Some(300), Some(240)),
        "list block after must keep 1.25rem/1rem hanging: {dock_p}"
    );
    assert_eq!(
        num_id(dock_p),
        Some(4),
        "list block after must keep checked task numId 4: {dock_p}"
    );
    assert_eq!(
        num_id(last_task_p),
        Some(3),
        "list block after must keep unchecked task numId 3: {last_task_p}"
    );
    assert_eq!(
        num_id(last_num_p),
        Some(2),
        "list block after must keep decimal numbering: {last_num_p}"
    );

    let list_style = style_block(&styles_xml, "ListParagraph").expect("ListParagraph style");
    assert_eq!(
        spacing_after_twips(list_style),
        Some(84),
        "ListParagraph style after must stay consecutive 0.35rem: {list_style}"
    );
    assert!(
        !list_style.contains("<w:contextualSpacing"),
        "Word contextualSpacing jams consecutive li vs WeasyPrint 0.35rem: {list_style}"
    );
    assert_eq!(
        ind_left_twips(list_style),
        Some(300),
        "list block after must keep ListParagraph 1.25rem indent: {list_style}"
    );

    let quote_p = paragraph_containing(&document_xml, "Replay is evidence")
        .expect("blockquote paragraph");
    assert!(
        ind_left_twips(quote_p).unwrap_or(0) >= 720
            && quote_p.contains("<w:pBdr>")
            && quote_p.contains(r#"w:color="134E4A""#)
            && p_bdr_left_sz(quote_p).unwrap_or(0) >= 24,
        "list block after must keep Quote 0.5in + 4px #134e4a bar: {quote_p}"
    );
    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains(r#"<w:vAlign w:val="top"/>"#),
        "list block after must keep th teal fill + valign top: {hop_tc}"
    );
    let body_p = paragraph_containing(&document_xml, "6 September 2026")
        .expect("body date paragraph");
    assert_eq!(
        spacing_attr(body_p, "w:line"),
        Some(276),
        "list block after must keep Word 1.15 (276): {body_p}"
    );
    let h1_p = paragraph_containing(&document_xml, r#"w:val="Heading1""#)
        .expect("Heading1 paragraph");
    assert!(
        !h1_p.contains("<w:sz "),
        "list block after must not reintroduce markdown CSS w:sz: {h1_p}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
}

#[test]
fn letter_md_docx_nested_list_gap_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("- [x] Receiving dock is clear")
            && text.contains("  - Bay 4")
            && text.contains("2. Run Convert")
            && text.contains("   - PDF/^A^"),
        "letter.md must keep nested Bay 4 and nested PDF/HTML Word paints at 0.25rem"
    );
    assert!(
        text.contains("- [x] Capsule hashes travel with the files")
            && text.contains("1. Hash the input"),
        "letter.md must keep consecutive tasks/decimals at 0.35rem"
    );
    assert!(
        text.contains("- [ ] Replay the convert instead of hoping")
            && text.contains("3. Compare the capsule"),
        "letter.md must keep last-list 1rem after"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    assert!(
        doc.sections[0].body.iter().any(|b| match b {
            Block::Paragraph(p) => {
                p.numbering
                    .as_ref()
                    .is_some_and(|n| n.level == 0 && p.plain_text().contains("Receiving dock is clear"))
            }
            _ => false,
        }),
        "letter.md IR must keep the dock parent of nested Bay 4"
    );
    assert!(
        doc.sections[0].body.iter().any(|b| match b {
            Block::Paragraph(p) => {
                p.numbering
                    .as_ref()
                    .is_some_and(|n| n.level >= 1 && p.plain_text().contains("Bay 4"))
            }
            _ => false,
        }),
        "letter.md IR must keep nested Bay 4"
    );

    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut styles_xml = String::new();
    zip.by_name("word/styles.xml")
        .expect("word/styles.xml")
        .read_to_string(&mut styles_xml)
        .unwrap();

    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    let bay_p = paragraph_containing(&document_xml, "Bay 4").expect("nested list item");
    let capsule_p = paragraph_containing(&document_xml, "Capsule hashes travel with the files")
        .expect("middle task");
    let last_task_p = paragraph_containing(&document_xml, "Replay the convert instead of hoping")
        .expect("last task list item");
    let hash_p = paragraph_containing(&document_xml, "Hash the input")
        .expect("first decimal list item");
    let convert_p = paragraph_containing(&document_xml, "Run Convert")
        .expect("decimal parent of nested PDF/HTML");
    let last_num_p = paragraph_containing(&document_xml, "Compare the capsule")
        .expect("last decimal list item");
    assert_eq!(
        spacing_after_twips(dock_p),
        Some(60),
        "letter.md dock parent of Bay 4 must keep letter.css ul ul 0.25rem (60 twips), not sibling 0.35rem: {dock_p}"
    );
    assert_eq!(
        spacing_after_twips(convert_p),
        Some(60),
        "letter.md Convert parent of nested PDF/HTML must keep 0.25rem: {convert_p}"
    );
    assert!(
        !dock_p.contains(r#"w:after="84""#)
            && !dock_p.contains(r#"w:after="240""#)
            && !convert_p.contains(r#"w:after="84""#),
        "python-docx 0.35rem / last-item 1rem parent-of-nested is sparse vs WeasyPrint 0.25rem: {dock_p} {convert_p}"
    );
    assert_eq!(
        spacing_after_twips(bay_p),
        Some(84),
        "nested Bay 4 item must stay consecutive 0.35rem: {bay_p}"
    );
    assert_eq!(
        spacing_after_twips(capsule_p),
        Some(84),
        "consecutive Capsule task must stay 0.35rem: {capsule_p}"
    );
    assert_eq!(
        spacing_after_twips(hash_p),
        Some(84),
        "consecutive Hash decimal must stay 0.35rem: {hash_p}"
    );
    assert_eq!(
        spacing_after_twips(last_task_p),
        Some(240),
        "nested gap must keep last task 1rem: {last_task_p}"
    );
    assert_eq!(
        spacing_after_twips(last_num_p),
        Some(240),
        "nested gap must keep last decimal 1rem: {last_num_p}"
    );

    let list_style = style_block(&styles_xml, "ListParagraph").expect("ListParagraph style");
    assert_eq!(
        spacing_after_twips(list_style),
        Some(84),
        "ListParagraph style after must stay consecutive 0.35rem: {list_style}"
    );
    assert!(
        !list_style.contains("<w:contextualSpacing"),
        "Word contextualSpacing jams consecutive li vs WeasyPrint 0.35rem: {list_style}"
    );
    assert_eq!(
        (ind_left_twips(dock_p), ind_hanging_twips(dock_p)),
        (Some(300), Some(240)),
        "nested gap must keep 1.25rem/1rem hanging: {dock_p}"
    );
    assert_eq!(
        (ind_left_twips(bay_p), ind_hanging_twips(bay_p)),
        (Some(600), Some(240)),
        "nested gap must keep nested 2.5rem hanging: {bay_p}"
    );
    assert_eq!(
        num_id(dock_p),
        Some(4),
        "nested gap must keep checked task numId 4: {dock_p}"
    );
    assert_eq!(
        num_id(bay_p),
        Some(1),
        "nested Bay 4 under a task must stay a disc bullet: {bay_p}"
    );
    assert_eq!(
        num_id(convert_p),
        Some(2),
        "nested gap must keep decimal numbering: {convert_p}"
    );

    let quote_p = paragraph_containing(&document_xml, "Replay is evidence")
        .expect("blockquote paragraph");
    assert!(
        ind_left_twips(quote_p).unwrap_or(0) >= 720
            && quote_p.contains("<w:pBdr>")
            && quote_p.contains(r#"w:color="134E4A""#)
            && p_bdr_left_sz(quote_p).unwrap_or(0) >= 24,
        "nested gap must keep Quote 0.5in + 4px #134e4a bar: {quote_p}"
    );
    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains(r#"<w:vAlign w:val="top"/>"#),
        "nested gap must keep th teal fill + valign top: {hop_tc}"
    );
    let body_p = paragraph_containing(&document_xml, "6 September 2026")
        .expect("body date paragraph");
    assert_eq!(
        spacing_attr(body_p, "w:line"),
        Some(276),
        "nested gap must keep Word 1.15 (276): {body_p}"
    );
    assert_eq!(
        spacing_after_twips(body_p),
        Some(168),
        "nested 0.25rem must keep body letter.css 0.7rem: {body_p}"
    );
    let h1_p = paragraph_containing(&document_xml, r#"w:val="Heading1""#)
        .expect("Heading1 paragraph");
    let h2_p = paragraph_containing(&document_xml, r#"w:val="Heading2""#)
        .expect("Heading2 paragraph");
    assert_eq!(
        spacing_attr(h1_p, "w:line"),
        Some(288),
        "nested gap must keep heading 1.2 leading: {h1_p}"
    );
    assert!(
        !h1_p.contains("<w:sz ") && !h2_p.contains("<w:sz "),
        "nested gap must not reintroduce markdown CSS w:sz: {h1_p} {h2_p}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
}

#[test]
fn letter_md_docx_after_mark_gap_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("![Harbor mark]") && text.contains("mark.png"),
        "letter.md must keep the Harbor mark Word paints with letter img/p 0.7rem after"
    );
    assert!(
        text.contains("6 September 2026"),
        "letter.md must keep the body date after the mark"
    );
    assert!(
        text.contains("- [x] Receiving dock is clear") && text.contains("  - Bay 4"),
        "letter.md must keep nested Bay 4"
    );
    assert!(
        text.contains("- [ ] Replay the convert instead of hoping")
            && text.contains("3. Compare the capsule"),
        "letter.md must keep last-list 1rem after"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    assert!(
        doc.sections[0].body.iter().any(|b| match b {
            Block::Paragraph(p) => p.runs.iter().any(|r| {
                matches!(&r.content, RunContent::Inline(InlineObject::Image(_)))
            }),
            _ => false,
        }),
        "letter.md IR must keep the Harbor mark image-only paragraph"
    );

    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut styles_xml = String::new();
    zip.by_name("word/styles.xml")
        .expect("word/styles.xml")
        .read_to_string(&mut styles_xml)
        .unwrap();

    let mark_p = harbor_paragraph(&document_xml).expect("paragraph wrapping the Harbor a:blip");
    assert_eq!(
        spacing_before_twips(mark_p),
        Some(144),
        "letter.md Harbor mark must keep letter.css img 0.6rem (144 twips) before, not Word 0: {mark_p}"
    );
    assert_eq!(
        spacing_after_twips(mark_p),
        Some(168),
        "letter.md Harbor mark must keep letter.css img/p 0.7rem (168 twips), not Word 8pt: {mark_p}"
    );
    assert!(
        !mark_p.contains(r#"w:before="0""#)
            && !mark_p.contains(r#"w:before="16""#)
            && !mark_p.contains(r#"w:before="80""#),
        "Word 0 / IR 80 / H2 4pt before the mark is jammed vs WeasyPrint 0.6rem: {mark_p}"
    );
    assert!(
        !mark_p.contains(r#"w:after="160""#) && !mark_p.contains(r#"w:after="240""#),
        "python-docx Normal 8pt / last-item 1rem after the mark is jammed or sparse vs WeasyPrint 0.7rem: {mark_p}"
    );
    let body_p = paragraph_containing(&document_xml, "6 September 2026")
        .expect("body date paragraph");
    assert_eq!(
        spacing_after_twips(body_p),
        Some(168),
        "after-mark 0.7rem must keep body letter.css p 0.7rem: {body_p}"
    );
    assert_eq!(
        spacing_attr(body_p, "w:line"),
        Some(276),
        "after-mark must keep Word 1.15 (276): {body_p}"
    );

    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    let last_task_p = paragraph_containing(&document_xml, "Replay the convert instead of hoping")
        .expect("last task list item");
    let last_num_p = paragraph_containing(&document_xml, "Compare the capsule")
        .expect("last decimal list item");
    assert_eq!(
        spacing_after_twips(dock_p),
        Some(60),
        "after-mark must keep nested 0.25rem: {dock_p}"
    );
    assert_eq!(
        spacing_after_twips(last_task_p),
        Some(240),
        "after-mark must keep last task 1rem: {last_task_p}"
    );
    assert_eq!(
        spacing_after_twips(last_num_p),
        Some(240),
        "after-mark must keep last decimal 1rem: {last_num_p}"
    );
    assert_eq!(
        (ind_left_twips(dock_p), ind_hanging_twips(dock_p)),
        (Some(300), Some(240)),
        "after-mark must keep 1.25rem/1rem hanging: {dock_p}"
    );

    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains(r#"<w:vAlign w:val="top"/>"#),
        "after-mark must keep th teal fill + valign top: {hop_tc}"
    );
    let h1_p = paragraph_containing(&document_xml, r#"w:val="Heading1""#)
        .expect("Heading1 paragraph");
    let h2_p = paragraph_containing(&document_xml, r#"w:val="Heading2""#)
        .expect("Heading2 paragraph");
    assert_eq!(
        spacing_attr(h1_p, "w:line"),
        Some(288),
        "after-mark must keep heading 1.2 leading: {h1_p}"
    );
    assert!(
        !h1_p.contains("<w:sz ") && !h2_p.contains("<w:sz "),
        "after-mark must not reintroduce markdown CSS w:sz: {h1_p} {h2_p}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
    let table_grid = style_block(&styles_xml, "TableGrid").expect("TableGrid style");
    assert!(
        table_grid.contains(r#"<w:vAlign w:val="top"/>"#),
        "after-mark must keep TableGrid valign top: {table_grid}"
    );
}

#[test]
fn letter_md_docx_before_mark_gap_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("## Delivery confirmation")
            && text.contains("![Harbor mark]")
            && text.contains("mark.png"),
        "letter.md must keep H2 then the Harbor mark Word paints with letter img 0.6rem before"
    );
    assert!(
        text.contains("6 September 2026"),
        "letter.md must keep the body date after the mark"
    );
    assert!(
        text.contains("- [x] Receiving dock is clear") && text.contains("  - Bay 4"),
        "letter.md must keep nested Bay 4"
    );
    assert!(
        text.contains("- [ ] Replay the convert instead of hoping")
            && text.contains("3. Compare the capsule"),
        "letter.md must keep last-list 1rem after"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    assert!(
        doc.sections[0].body.iter().any(|b| match b {
            Block::Paragraph(p) => p.runs.iter().any(|r| {
                matches!(&r.content, RunContent::Inline(InlineObject::Image(_)))
            }),
            _ => false,
        }),
        "letter.md IR must keep the Harbor mark image-only paragraph"
    );

    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut styles_xml = String::new();
    zip.by_name("word/styles.xml")
        .expect("word/styles.xml")
        .read_to_string(&mut styles_xml)
        .unwrap();

    let mark_p = harbor_paragraph(&document_xml).expect("paragraph wrapping the Harbor a:blip");
    assert_eq!(
        spacing_before_twips(mark_p),
        Some(144),
        "letter.md Harbor mark must keep letter.css img 0.6rem (144 twips) before, not Word 0: {mark_p}"
    );
    assert!(
        !mark_p.contains(r#"w:before="0""#)
            && !mark_p.contains(r#"w:before="16""#)
            && !mark_p.contains(r#"w:before="80""#)
            && !mark_p.contains(r#"w:before="160""#)
            && !mark_p.contains(r#"w:before="168""#),
        "python-docx 0 / IR 80 / H2 4pt / Normal 8pt / after-mark 0.7rem before the mark is jammed or sparse vs WeasyPrint 0.6rem: {mark_p}"
    );
    assert_eq!(
        spacing_after_twips(mark_p),
        Some(168),
        "before-mark 0.6rem must keep letter.css img/p 0.7rem (168 twips) after: {mark_p}"
    );
    assert!(
        !mark_p.contains(r#"w:after="160""#) && !mark_p.contains(r#"w:after="240""#),
        "before-mark must keep after-mark 0.7rem, not Word 8pt or last-list 1rem: {mark_p}"
    );
    let body_p = paragraph_containing(&document_xml, "6 September 2026")
        .expect("body date paragraph");
    assert_eq!(
        spacing_after_twips(body_p),
        Some(168),
        "before-mark 0.6rem must keep body letter.css p 0.7rem: {body_p}"
    );
    assert_eq!(
        spacing_before_twips(body_p).unwrap_or(0),
        0,
        "before-mark 0.6rem must not leak onto the next body before: {body_p}"
    );
    assert_eq!(
        spacing_attr(body_p, "w:line"),
        Some(276),
        "before-mark must keep Word 1.15 (276): {body_p}"
    );

    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    let last_task_p = paragraph_containing(&document_xml, "Replay the convert instead of hoping")
        .expect("last task list item");
    let last_num_p = paragraph_containing(&document_xml, "Compare the capsule")
        .expect("last decimal list item");
    assert_eq!(
        spacing_after_twips(dock_p),
        Some(60),
        "before-mark must keep nested 0.25rem: {dock_p}"
    );
    assert_eq!(
        spacing_after_twips(last_task_p),
        Some(240),
        "before-mark must keep last task 1rem: {last_task_p}"
    );
    assert_eq!(
        spacing_after_twips(last_num_p),
        Some(240),
        "before-mark must keep last decimal 1rem: {last_num_p}"
    );
    assert_eq!(
        (ind_left_twips(dock_p), ind_hanging_twips(dock_p)),
        (Some(300), Some(240)),
        "before-mark must keep 1.25rem/1rem hanging: {dock_p}"
    );

    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains(r#"<w:vAlign w:val="top"/>"#),
        "before-mark must keep th teal fill + valign top: {hop_tc}"
    );
    let h1_p = paragraph_containing(&document_xml, r#"w:val="Heading1""#)
        .expect("Heading1 paragraph");
    let h2_p = paragraph_containing(&document_xml, r#"w:val="Heading2""#)
        .expect("Heading2 paragraph");
    assert_eq!(
        spacing_attr(h1_p, "w:line"),
        Some(288),
        "before-mark must keep heading 1.2 leading: {h1_p}"
    );
    assert_eq!(
        spacing_attr(h2_p, "w:line"),
        Some(288),
        "before-mark must keep H2 1.2 leading: {h2_p}"
    );
    assert!(
        spacing_before_twips(h2_p).unwrap_or(0) >= 200,
        "before-mark 0.6rem must not steal Heading2 10pt before: {h2_p}"
    );
    assert!(
        !h1_p.contains("<w:sz ") && !h2_p.contains("<w:sz "),
        "before-mark must not reintroduce markdown CSS w:sz: {h1_p} {h2_p}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
    let table_grid = style_block(&styles_xml, "TableGrid").expect("TableGrid style");
    assert!(
        table_grid.contains(r#"<w:vAlign w:val="top"/>"#),
        "before-mark must keep TableGrid valign top: {table_grid}"
    );
}

#[test]
fn letter_md_docx_page_margin_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("# Northwind Freight") && text.contains("| Hop | File | Proof |"),
        "letter.md must keep H1 and the hop table Word paints inside @page 2.5rem/2.75rem"
    );
    assert!(
        text.contains("6 September 2026"),
        "letter.md must keep the body date"
    );
    assert!(
        text.contains("> Replay is evidence. Same fonts, same bytes."),
        "letter.md must keep the blockquote"
    );
    assert!(
        text.contains("- [x] Receiving dock is clear"),
        "letter.md must keep tasks"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    assert_eq!(
        doc.sections[0].page.margin_left,
        docagent_model::DEFAULT_MARGIN_HU,
        "letter.md IR must keep Hangul DEFAULT_MARGIN so the codec can remap @page"
    );
    let hangul_twips = i64::from(docagent_model::DEFAULT_MARGIN_HU / 5);
    let letter_x = 660i64;
    let letter_y = 600i64;

    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();

    assert_eq!(
        (
            pg_mar_attr(&document_xml, "w:left"),
            pg_mar_attr(&document_xml, "w:right"),
            pg_mar_attr(&document_xml, "w:top"),
            pg_mar_attr(&document_xml, "w:bottom")
        ),
        (Some(letter_x), Some(letter_x), Some(letter_y), Some(letter_y)),
        "letter pgMar must be @page 2.5rem/2.75rem (600/660 twips), not Hangul 30mm: {document_xml}"
    );
    assert!(
        pg_mar_attr(&document_xml, "w:left") != Some(hangul_twips)
            && pg_mar_attr(&document_xml, "w:top") != Some(hangul_twips)
            && pg_mar_attr(&document_xml, "w:left") != Some(1440)
            && pg_mar_attr(&document_xml, "w:left") != Some(1125),
        "Hangul 30mm / Word 1in / WeasyPrint UA 75px is a sparse Word frame: {document_xml}"
    );
    assert_eq!(
        (
            pg_mar_attr(&document_xml, "w:header"),
            pg_mar_attr(&document_xml, "w:footer")
        ),
        (Some(letter_y / 2), Some(letter_y / 2)),
        "header/footer must sit inside letter 2.5rem, not Hangul 15mm: {document_xml}"
    );

    let hop_width = tbl_w_twips(&document_xml).expect("w:tblW on the hop table");
    let hangul_measure = i64::from(
        (doc.sections[0].page.width - 2 * docagent_model::DEFAULT_MARGIN_HU) / 5,
    );
    assert!(
        hop_width > hangul_measure,
        "hop table must span the letter 2.75rem measure, not Hangul 30mm gutters, got {hop_width} vs {hangul_measure}: {document_xml}"
    );

    let body_p = paragraph_containing(&document_xml, "6 September 2026")
        .expect("body date paragraph");
    let body_run = run_containing(body_p, ">6 September 2026</w:t>").expect("body date run");
    assert!(
        body_run.contains(r#"w:ascii="Segoe UI""#)
            && body_run.contains(r#"<w:color w:val="134E4A"/>"#),
        "page margin must keep Segoe UI body #134e4a: {body_run}"
    );
    assert_eq!(
        spacing_attr(body_p, "w:line"),
        Some(276),
        "page margin must keep Word 1.15 (276): {body_p}"
    );
    let quote_p = paragraph_containing(&document_xml, "Replay is evidence")
        .expect("blockquote paragraph");
    assert!(
        quote_p.contains("<w:pBdr>")
            && quote_p.contains("<w:left ")
            && quote_p.contains(r#"w:color="134E4A""#)
            && p_bdr_left_sz(quote_p).unwrap_or(0) >= 24,
        "page margin must keep quote 4px #134e4a left bar: {quote_p}"
    );
    assert!(
        ind_left_twips(quote_p).unwrap_or(0) >= 720,
        "page margin must keep Quote 0.5in: {quote_p}"
    );
    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains("<w:tcBorders>"),
        "page margin must keep th teal fill: {hop_tc}"
    );
    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    assert_eq!(
        (ind_left_twips(dock_p), ind_hanging_twips(dock_p)),
        (Some(300), Some(240)),
        "page margin must keep 1.25rem/1rem list indent: {dock_p}"
    );
    assert_eq!(
        num_id(dock_p),
        Some(4),
        "page margin must keep checked task numId 4: {dock_p}"
    );
    let h1_p = paragraph_containing(&document_xml, r#"w:val="Heading1""#)
        .expect("Heading1 paragraph");
    let h2_p = paragraph_containing(&document_xml, r#"w:val="Heading2""#)
        .expect("Heading2 paragraph");
    assert_eq!(
        spacing_attr(h1_p, "w:line"),
        Some(288),
        "page margin must keep heading 1.2 leading: {h1_p}"
    );
    assert!(
        !h1_p.contains("<w:sz ") && !h2_p.contains("<w:sz "),
        "page margin must not reintroduce markdown CSS w:sz: {h1_p} {h2_p}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
}

#[test]
fn letter_md_docx_mark_color_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("==three hashes=="),
        "letter.md must keep the ==highlight== Word paints as Mark teal on amber"
    );
    assert!(
        text.contains("`prove`") && text.contains("- [x] Receiving dock is clear"),
        "letter.md must keep the prove chip and tasks"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    assert!(
        doc.sections[0].body.iter().any(|b| match b {
            Block::Paragraph(p) => p.runs.iter().any(|r| {
                r.style.highlight.is_some()
                    && matches!(&r.content, RunContent::Text(t) if t == "three hashes")
            }),
            _ => false,
        }),
        "letter.md IR must keep ==three hashes== as highlight"
    );

    let docx = docagent_docx::write(&doc).expect("docx write");
    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut styles_xml = String::new();
    zip.by_name("word/styles.xml")
        .expect("word/styles.xml")
        .read_to_string(&mut styles_xml)
        .unwrap();

    let flow_p = paragraph_containing(&document_xml, "three hashes")
        .expect("letter mark paragraph");
    let mark_run = run_containing(flow_p, ">three hashes</w:t>").expect("mark run");
    assert!(
        mark_run.contains(r#"w:val="Mark""#)
            && mark_run.contains(r#"<w:highlight w:val="yellow"/>"#)
            && mark_run.contains(r#"w:fill="FDE68A""#),
        "letter ==three hashes== must stay a visible Word highlight + letter amber: {mark_run}"
    );
    assert!(
        mark_run.contains(r#"<w:color w:val="134E4A"/>"#)
            && !mark_run.contains(r#"w:val="000000""#)
            && !mark_run.contains(r#"w:color="auto""#)
            && !mark_run.contains(r#"w:val="111827""#),
        "letter mark type must stay letter.html #134e4a, not highlighter black-on-yellow: {mark_run}"
    );
    assert!(
        bdr_space_for_color(flow_p, "FDE68A").unwrap_or(0) >= 2,
        "mark color must keep Word rPr padding (.05em/.2em), got {:?}: {flow_p}",
        bdr_space_for_color(flow_p, "FDE68A")
    );

    let mark_char = style_block(&styles_xml, "Mark").expect("Mark character style");
    assert!(
        mark_char.contains(r#"w:val="yellow""#) && mark_char.contains(r#"w:fill="FDE68A""#),
        "Mark style must stay Word highlight + letter amber: {mark_char}"
    );
    assert!(
        mark_char.contains(r#"<w:color w:val="134E4A"/>"#)
            && !mark_char.contains(r#"w:val="000000""#)
            && !mark_char.contains(r#"w:color="auto""#)
            && !mark_char.contains(r#"w:val="111827""#),
        "Mark style type must stay letter.html #134e4a so style-only Mark is not black-on-yellow: {mark_char}"
    );
    assert!(
        mark_char.contains(r#"w:ascii="Segoe UI""#) && !mark_char.contains("Calibri"),
        "Mark style must keep letter Segoe UI, not theme Calibri: {mark_char}"
    );

    let prove_run = run_containing(flow_p, ">prove</w:t>").expect("prove code run");
    assert_eq!(
        style_sz(prove_run).expect("prove w:sz"),
        20,
        "mark color must keep inline `prove` at Word 10pt: {prove_run}"
    );
    assert!(
        prove_run.contains(r#"w:val="134E4A""#) && prove_run.contains(r#"w:fill="CCFBF1""#),
        "mark color must keep the teal Consolas chip: {prove_run}"
    );

    let body_p = paragraph_containing(&document_xml, "6 September 2026")
        .expect("body date paragraph");
    let body_run = run_containing(body_p, ">6 September 2026</w:t>").expect("body date run");
    assert!(
        body_run.contains(r#"<w:color w:val="134E4A"/>"#)
            && !body_run.contains(r#"w:fill="FDE68A""#),
        "mark color must keep body #134e4a and not leak amber onto Normal: {body_run}"
    );
    assert_eq!(
        spacing_attr(body_p, "w:line"),
        Some(276),
        "mark color must keep Word 1.15 (276): {body_p}"
    );

    let quote_p = paragraph_containing(&document_xml, "Replay is evidence")
        .expect("blockquote paragraph");
    assert!(
        quote_p.contains("<w:pBdr>")
            && quote_p.contains("<w:left ")
            && quote_p.contains(r#"w:color="134E4A""#)
            && p_bdr_left_sz(quote_p).unwrap_or(0) >= 24,
        "mark color must keep quote 4px #134e4a left bar: {quote_p}"
    );
    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains("<w:tcBorders>"),
        "mark color must keep th teal fill: {hop_tc}"
    );
    let dock_p = paragraph_containing(&document_xml, "Receiving dock is clear")
        .expect("checked task");
    assert_eq!(
        num_id(dock_p),
        Some(4),
        "mark color must keep checked task numId 4: {dock_p}"
    );
    let h1_p = paragraph_containing(&document_xml, r#"w:val="Heading1""#)
        .expect("Heading1 paragraph");
    let h2_p = paragraph_containing(&document_xml, r#"w:val="Heading2""#)
        .expect("Heading2 paragraph");
    assert!(
        !h1_p.contains("<w:sz ") && !h2_p.contains("<w:sz "),
        "mark color must not reintroduce markdown CSS w:sz on headings: {h1_p} {h2_p}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must keep the 24pt floor, got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "24pt a:blip must not regress: {document_xml}"
    );
}

#[test]
fn letter_md_docx_word_package_like_pandoc() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("# Northwind Freight") && text.contains("| Hop | File | Proof |"),
        "letter.md must keep H1 and the hop table Word paints"
    );
    assert!(
        text.contains("!["),
        "letter.md must keep the Harbor mark Word paints as a:blip"
    );
    assert!(
        text.contains("mark.png"),
        "letter.md must reference mark.png"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    let docx = docagent_docx::write(&doc).expect("docx write");
    assert!(docagent_docx::sniff(&docx), "written bytes must sniff as DOCX");

    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("word/document.xml")
        .read_to_string(&mut document_xml)
        .unwrap();
    let mut styles_xml = String::new();
    zip.by_name("word/styles.xml")
        .expect("word/styles.xml")
        .read_to_string(&mut styles_xml)
        .unwrap();
    let mut settings_xml = String::new();
    zip.by_name("word/settings.xml")
        .expect("word/settings.xml")
        .read_to_string(&mut settings_xml)
        .unwrap();
    let mut rels_xml = String::new();
    zip.by_name("word/_rels/document.xml.rels")
        .expect("word/_rels/document.xml.rels")
        .read_to_string(&mut rels_xml)
        .unwrap();

    assert!(
        document_xml.contains(r#"w:val="Heading1""#)
            && document_xml.contains(r#"w:val="Heading2""#),
        "DOCX from letter.md must use Word heading styles: {document_xml}"
    );
    assert!(
        document_xml.contains("<w:tbl>") && document_xml.contains("<w:tblBorders>"),
        "DOCX from letter.md must include a visible table: {document_xml}"
    );
    assert!(
        document_xml.contains(r#"<w:tblCellSpacing w:w="0" w:type="dxa"/>"#)
            && !document_xml.contains(r#"<w:tblCellSpacing w:w="40""#),
        "letter hop must be letter.html border-collapse, not Word 2003 2px gutters: {document_xml}"
    );
    assert!(
        document_xml.contains("<a:blip r:embed=")
            && document_xml.contains(
                r#"<pic:pic xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture">"#
            )
            && document_xml.contains(
                r#"<a:graphic xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">"#
            ),
        "python-docx xmlns:a/pic so Word paints a:blip: {document_xml}"
    );
    let (cx, cy) = extent_cx_cy(&document_xml).expect("wp:extent on the Harbor drawing");
    assert!(
        cx >= MIN_VISIBLE_EMU && cy >= MIN_VISIBLE_EMU,
        "Harbor a:blip must be at least 24pt ({MIN_VISIBLE_EMU} EMU), got {cx}×{cy}"
    );
    assert!(
        document_xml.contains("Harbor mark"),
        "Harbor mark alt must appear on wp:docPr: {document_xml}"
    );

    assert!(
        settings_xml.contains(r#"w:name="compatibilityMode""#)
            && settings_xml.contains(r#"w:val="15""#)
            && settings_xml.contains("overrideTableStyleFontSizeAndJustification")
            && settings_xml.contains(r#"<w:characterSpacingControl w:val="doNotCompress"/>"#),
        "python-docx/Pandoc Word 2013 settings so heading tracking + table sz paint: {settings_xml}"
    );
    assert!(
        rels_xml.contains("/relationships/settings")
            && rels_xml.contains("Target=\"settings.xml\"")
            && rels_xml.contains("/relationships/image"),
        "settings rel must not steal the Harbor a:blip rId: {rels_xml}"
    );

    assert!(
        styles_xml.contains("<w:docDefaults>")
            && styles_xml.contains(r#"w:ascii="Segoe UI""#)
            && styles_xml.contains(r#"<w:color w:val="134E4A"/>"#)
            && styles_xml.contains(r#"w:line="276""#),
        "docDefaults must stay letter Segoe UI 11pt #134e4a line 276: {styles_xml}"
    );
    let table_grid = style_block(&styles_xml, "TableGrid").expect("TableGrid style");
    assert!(
        table_grid.contains(r#"<w:tblCellSpacing w:w="0" w:type="dxa"/>"#)
            && table_grid.contains(r#"w:color="CBD5E1""#)
            && table_grid.contains(r#"w:fill="CCFBF1""#),
        "TableGrid must keep collapse + letter slate/teal: {table_grid}"
    );

    let h1_p = paragraph_containing(&document_xml, r#"w:val="Heading1""#)
        .expect("Heading1 paragraph");
    let h2_p = paragraph_containing(&document_xml, r#"w:val="Heading2""#)
        .expect("Heading2 paragraph");
    assert!(
        !h1_p.contains("<w:sz ") && !h2_p.contains("<w:sz "),
        "letter H1/H2 must not override Word 16pt/13pt: {h1_p} {h2_p}"
    );
    assert_eq!(
        spacing_attr(h1_p, "w:line"),
        Some(288),
        "package must keep heading 1.2 leading: {h1_p}"
    );
    let hop_tc = cell_containing(&document_xml, "Hop").expect("Hop header cell");
    assert!(
        hop_tc.contains(r#"w:fill="CCFBF1""#)
            && hop_tc.contains(r#"w:color="0D9488""#)
            && hop_tc.contains("<w:tcBorders>"),
        "package must keep th teal fill: {hop_tc}"
    );
    let body_p = paragraph_containing(&document_xml, "6 September 2026")
        .expect("body date paragraph");
    assert_eq!(
        spacing_attr(body_p, "w:line"),
        Some(276),
        "package must keep Word 1.15: {body_p}"
    );
}

fn extent_cx_cy(xml: &str) -> Option<(i64, i64)> {
    let start = xml.find("<wp:extent ")?;
    let tag = xml[start..].split('>').next()?;
    let cx = attr_i64(tag, "cx")?;
    let cy = attr_i64(tag, "cy")?;
    Some((cx, cy))
}

fn harbor_paragraph(xml: &str) -> Option<&str> {
    let blip = xml.find("<a:blip r:embed=")?;
    let start = xml[..blip].rfind("<w:p")?;
    let end = xml[blip..].find("</w:p>")?;
    Some(&xml[start..blip + end + 6])
}

fn spacing_after_twips(p: &str) -> Option<i64> {
    spacing_attr(p, "w:after")
}

fn spacing_before_twips(p: &str) -> Option<i64> {
    spacing_attr(p, "w:before")
}

fn spacing_attr(p: &str, key: &str) -> Option<i64> {
    let start = p.find("<w:spacing ")?;
    let tag = p[start..].split('>').next()?;
    attr_i64(tag, key)
}

fn paragraph_containing<'a>(xml: &'a str, needle: &str) -> Option<&'a str> {
    let at = xml.find(needle)?;
    let start = p_tag_start(&xml[..at])?;
    let end = xml[at..].find("</w:p>")?;
    Some(&xml[start..at + end + 6])
}

/// `<w:p>` / `<w:p …>`, not `<w:pBdr>` / `<w:pPr>` / `<w:pStyle>`.
fn p_tag_start(xml: &str) -> Option<usize> {
    let mut search_from = xml.len();
    while let Some(i) = xml[..search_from].rfind("<w:p") {
        let after = xml.get(i + 4..)?;
        if after.starts_with('>') || after.starts_with(' ') || after.starts_with('/') {
            return Some(i);
        }
        search_from = i;
    }
    None
}

fn run_containing<'a>(xml: &'a str, needle: &str) -> Option<&'a str> {
    let at = xml.find(needle)?;
    let start = xml[..at]
        .rfind("<w:r>")
        .or_else(|| xml[..at].rfind("<w:r "))?;
    let end = xml[at..].find("</w:r>")?;
    Some(&xml[start..at + end + 6])
}

fn cell_containing<'a>(xml: &'a str, needle: &str) -> Option<&'a str> {
    let at = xml.find(needle)?;
    let start = xml[..at]
        .rfind("<w:tc>")
        .or_else(|| xml[..at].rfind("<w:tc "))?;
    let end = xml[at..].find("</w:tc>")?;
    Some(&xml[start..at + end + 7])
}

fn tbl_borders_block(xml: &str) -> Option<&str> {
    let start = xml.find("<w:tblBorders>")?;
    let rest = &xml[start..];
    let end = rest.find("</w:tblBorders>")?;
    Some(&rest[..end])
}

fn tc_bottom_sz(fragment: &str) -> Option<i64> {
    let start = fragment.find("<w:tcBorders>")?;
    let rest = &fragment[start..];
    let end = rest.find("</w:tcBorders>")?;
    let block = &rest[..end];
    let tag_start = block.find("<w:bottom ")?;
    let tag = block[tag_start..].split('>').next()?;
    attr_i64(tag, "w:sz")
}

fn style_block<'a>(xml: &'a str, style_id: &str) -> Option<&'a str> {
    let needle = format!(r#"w:styleId="{style_id}""#);
    let start = xml.find(&needle)?;
    let rest = &xml[start..];
    let end = rest.find("</w:style>")?;
    Some(&rest[..end])
}

fn style_sz(fragment: &str) -> Option<i64> {
    let start = fragment.find("<w:sz ")?;
    let tag = fragment[start..].split('>').next()?;
    attr_i64(tag, "w:val")
}

fn ind_left_twips(fragment: &str) -> Option<i64> {
    let start = fragment.find("<w:ind ")?;
    let tag = fragment[start..].split('>').next()?;
    attr_i64(tag, "w:left")
}

fn ind_hanging_twips(fragment: &str) -> Option<i64> {
    let start = fragment.find("<w:ind ")?;
    let tag = fragment[start..].split('>').next()?;
    attr_i64(tag, "w:hanging")
}

fn ind_right_twips(fragment: &str) -> Option<i64> {
    let start = fragment.find("<w:ind ")?;
    let tag = fragment[start..].split('>').next()?;
    attr_i64(tag, "w:right")
}

fn num_id(fragment: &str) -> Option<i64> {
    let start = fragment.find("<w:numId ")?;
    let tag = fragment[start..].split('>').next()?;
    attr_i64(tag, "w:val")
}

fn p_bdr_bottom_sz(fragment: &str) -> Option<i64> {
    let start = fragment.find("<w:bottom ")?;
    let tag = fragment[start..].split('>').next()?;
    attr_i64(tag, "w:sz")
}

fn p_bdr_top_sz(fragment: &str) -> Option<i64> {
    let start = fragment.find("<w:top ")?;
    let tag = fragment[start..].split('>').next()?;
    attr_i64(tag, "w:sz")
}

fn p_bdr_left_sz(fragment: &str) -> Option<i64> {
    let start = fragment.find("<w:left ")?;
    let tag = fragment[start..].split('>').next()?;
    attr_i64(tag, "w:sz")
}

fn bdr_space_for_color(fragment: &str, color: &str) -> Option<i64> {
    let mut rest = fragment;
    let color_needle = format!(r#"w:color="{color}""#);
    while let Some(start) = rest.find("<w:bdr ") {
        let tag = rest[start..].split('>').next()?;
        if tag.contains(&color_needle) {
            return attr_i64(tag, "w:space");
        }
        rest = &rest[start + 7..];
    }
    None
}

fn mar_side_twips(xml: &str, container: &str, side: &str) -> Option<i64> {
    let open = format!("<{container}>");
    let close = format!("</{container}>");
    let start = xml.find(&open)?;
    let rest = &xml[start..];
    let end = rest.find(&close)?;
    let block = &rest[..end];
    let needle = format!("<w:{side} ");
    let tag = block.split(&needle).nth(1)?.split('>').next()?;
    attr_i64(tag, "w:w")
}

fn attr_i64(tag: &str, key: &str) -> Option<i64> {
    let needle = format!("{key}=\"");
    let rest = tag.split(&needle).nth(1)?;
    rest.split('"').next()?.parse().ok()
}

fn pg_mar_attr(xml: &str, key: &str) -> Option<i64> {
    let start = xml.find("<w:pgMar ")?;
    let tag = xml[start..].split('>').next()?;
    attr_i64(tag, key)
}

fn tbl_w_twips(xml: &str) -> Option<i64> {
    let start = xml.find("<w:tblW ")?;
    let tag = xml[start..].split('>').next()?;
    attr_i64(tag, "w:w")
}
