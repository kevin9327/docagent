//! `examples/letter.md` → ODT must look like LibreOffice: heading, table,
//! and a visible `draw:image` Harbor PNG (24pt floor, not 16px at 96dpi).

use std::io::{Cursor, Read};
use std::path::Path;

use docagent_model::{Block, InlineObject, NumberFormat, RunContent, A4_WIDTH_HU, DEFAULT_MARGIN_HU};
use zip::ZipArchive;

const PNG_MAGIC: &[u8] = &[0x89, b'P', b'N', b'G'];
/// 24pt in HWPUNIT (1/7200 in). Matches the layout on-page image floor.
const MIN_IMAGE_EDGE_HU: i32 = 24 * 100;

#[test]
fn letter_md_odt_looks_like_libreoffice() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("!["),
        "examples/letter.md must contain a markdown image"
    );
    assert!(
        text.contains("mark.png"),
        "examples/letter.md must reference docs/assets/mark.png"
    );
    assert!(
        text.contains("---"),
        "examples/letter.md must keep the thematic break"
    );
    assert!(
        text.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
        "examples/letter.md must keep the DocAgent hyperlink"
    );
    assert!(
        text.contains("`prove`"),
        "examples/letter.md must keep the inline code span Writer paints as a chip"
    );
    assert!(
        text.contains("==three hashes=="),
        "examples/letter.md must keep the ==highlight== Writer paints as Tmark"
    );
    assert!(
        text.contains("~~guess~~"),
        "examples/letter.md must keep the strikethrough Writer paints as Tstrike"
    );
    assert!(
        text.contains("__09:00 local__"),
        "examples/letter.md must keep the underline Writer paints as Tunder"
    );
    assert!(
        text.contains("PDF/^A^"),
        "examples/letter.md must keep the PDF/A superscript Writer paints as Tsuper"
    );
    assert!(
        text.contains("H~2~O"),
        "examples/letter.md must keep the H2O subscript Writer paints as Tsub"
    );
    assert!(
        text.contains("- [x] Receiving dock is clear")
            && text.contains("- [ ] Replay the convert instead of hoping"),
        "examples/letter.md must keep - [x] / - [ ] task items"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    let odt = docagent_odt::write(&doc).expect("odt write");
    assert!(docagent_odt::sniff(&odt), "written bytes must sniff as ODT");

    let mut zip = ZipArchive::new(Cursor::new(odt)).expect("odt zip");
    let mut content = String::new();
    zip.by_name("content.xml")
        .expect("content.xml")
        .read_to_string(&mut content)
        .unwrap();
    let mut styles = String::new();
    zip.by_name("styles.xml")
        .expect("styles.xml")
        .read_to_string(&mut styles)
        .unwrap();
    assert!(
        styles.contains(r#"style:display-name="Heading 1""#)
            && styles.contains(r#"style:default-outline-level="1""#)
            && styles.contains(r#"style:name="Heading_20_1""#),
        "styles.xml must include Writer Heading 1: {styles}"
    );
    let heading_parent = office_style_xml(&styles, "Heading");
    let heading1_named = office_style_xml(&styles, "Heading_20_1");
    let heading2_named = office_style_xml(&styles, "Heading_20_2");
    let heading3_named = office_style_xml(&styles, "Heading_20_3");
    assert!(
        heading_parent.contains("fo:color=\"#134e4a\"")
            && heading1_named.contains("fo:color=\"#134e4a\"")
            && heading2_named.contains("fo:color=\"#134e4a\"")
            && heading3_named.contains("fo:color=\"#134e4a\"")
            && heading_parent.contains(r#"style:use-window-font-color="false""#)
            && heading1_named.contains(r#"style:use-window-font-color="false""#)
            && heading2_named.contains(r#"style:use-window-font-color="false""#)
            && heading3_named.contains(r#"style:use-window-font-color="false""#)
            && !styles.contains("fo:color=\"#000080\"")
            && !styles.contains(r#"style:use-window-font-color="true""#),
        "Writer Heading parent/1/2/3 must stay letter #134e4a, not Liberation navy: {heading_parent}"
    );
    assert!(
        styles.contains(r#"style:display-name="Heading 2""#)
            && styles.contains(r#"style:name="Graphics""#)
            && styles.contains(r#"style:display-name="Text body""#),
        "styles.xml must include Writer Heading 2, Graphics, Text body: {styles}"
    );
    assert!(
        styles.contains(r#"draw:fill="none""#)
            && styles.contains(r#"draw:image-opacity="100%""#)
            && styles.contains(r#"style:background-transparency="100%""#)
            && styles.contains(r#"draw:color-mode="standard""#),
        "Writer Graphics must not fill over draw:image: {styles}"
    );
    assert!(
        styles.contains(r#"style:master-page style:name="Standard""#)
            && styles.contains("fo:page-width=")
            && styles.contains("fo:page-height="),
        "styles.xml must include a Writer A4 master page: {styles}"
    );
    let page = page_layout_xml(&styles);
    let margin_x = odf_len_hu(attr(page, "fo:margin-left").expect("fo:margin-left on page-layout"));
    let margin_y = odf_len_hu(attr(page, "fo:margin-top").expect("fo:margin-top on page-layout"));
    let margin_right =
        odf_len_hu(attr(page, "fo:margin-right").expect("fo:margin-right on page-layout"));
    let margin_bottom =
        odf_len_hu(attr(page, "fo:margin-bottom").expect("fo:margin-bottom on page-layout"));
    assert!(
        (3100..3500).contains(&margin_x)
            && (3100..3500).contains(&margin_right)
            && (2800..3200).contains(&margin_y)
            && (2800..3200).contains(&margin_bottom)
            && margin_x == margin_right
            && margin_y == margin_bottom
            && margin_x > margin_y,
        "Writer page-layout must be letter @page 2.5rem/2.75rem (3000/3300 HU), not Hangul 30mm, got {margin_y}/{margin_x}: {page}"
    );
    assert!(
        margin_x != DEFAULT_MARGIN_HU
            && margin_y != DEFAULT_MARGIN_HU
            && margin_x != 75 * 75
            && margin_y != 75 * 75,
        "Hangul 30mm / WeasyPrint UA 75px is a sparse Writer frame: {page}"
    );
    let default_p = default_style_xml(&styles, "paragraph");
    assert!(
        default_p.contains("fo:color=\"#134e4a\"")
            && default_p.contains(r#"style:use-window-font-color="false""#)
            && !default_p.contains("fo:color=\"#000000\"")
            && !default_p.contains("fo:color=\"#111827\""),
        "Writer default paragraph must be letter teal, not UA black: {default_p}"
    );
    let standard = office_style_xml(&styles, "Standard");
    assert!(
        standard.contains("fo:color=\"#134e4a\"")
            && standard.contains(r#"style:use-window-font-color="false""#)
            && !standard.contains("fo:color=\"#000000\"")
            && !standard.contains("fo:color=\"#111827\""),
        "Writer Standard must stay letter #134e4a so style-only body is not UA black: {standard}"
    );
    assert!(
        standard.contains(r#"style:font-name="Segoe UI""#)
            && standard.contains(r#"fo:font-family="Segoe UI""#)
            && !standard.contains(r#"style:font-name="Liberation Sans""#),
        "Writer Standard must stay letter Segoe UI, not Liberation Sans: {standard}"
    );
    let text_body = office_style_xml(&styles, "Text_20_body");
    assert!(
        text_body.contains("fo:color=\"#134e4a\"") && !text_body.contains("fo:color=\"#111827\""),
        "Writer Text body must stay letter #134e4a: {text_body}"
    );

    assert!(
        content.contains("<text:h"),
        "ODT from letter.md must include a heading: {content}"
    );
    assert!(
        content.contains(r#"text:style-name="Heading1""#)
            || content.contains(r#"text:outline-level="1""#),
        "ODT heading must be outline level 1: {content}"
    );
    assert!(
        content.contains("Northwind Freight"),
        "ODT must keep the letter heading text: {content}"
    );
    let heading1 = style_xml(&content, "Heading1", "paragraph");
    assert!(
        heading1.contains(r#"fo:font-size="18pt""#),
        "Heading1 must be Writer 18pt (130% of 14pt), got: {heading1}"
    );
    let h1_before = odf_len_hu(
        attr(heading1, "fo:margin-top").expect("fo:margin-top on Heading1"),
    );
    let h1_after = odf_len_hu(
        attr(heading1, "fo:margin-bottom").expect("fo:margin-bottom on Heading1"),
    );
    assert!(
        h1_before == 0,
        "Heading1 before must be letter.css h1 margin-top 0, not Writer 0.1665in {h1_before}: {heading1}"
    );
    assert!(
        !heading1.contains(r#"fo:margin-top="0.1665in""#),
        "Heading1 must not keep Writer 0.1665in before: {heading1}"
    );
    assert!(
        (680..760).contains(&h1_after),
        "Heading1 after must be letter 0.6rem (~720 HU), not Writer 0.0835in {h1_after}: {heading1}"
    );
    assert!(
        !heading1.contains(r#"fo:margin-bottom="0.0835in""#),
        "Heading1 must not keep Writer 0.0835in after: {heading1}"
    );
    let named_h1_after = odf_len_hu(
        attr(heading1_named, "fo:margin-bottom").expect("fo:margin-bottom on Heading 1"),
    );
    assert!(
        (680..760).contains(&named_h1_after)
            && !heading1_named.contains(r#"fo:margin-bottom="0.0835in""#),
        "Writer Heading 1 sidebar must keep letter 0.6rem after, got {named_h1_after}: {heading1_named}"
    );
    let named_h1_before = odf_len_hu(
        attr(heading1_named, "fo:margin-top").expect("fo:margin-top on Heading 1"),
    );
    assert!(
        named_h1_before == 0 && !heading1_named.contains(r#"fo:margin-top="0.1665in""#),
        "Writer Heading 1 sidebar must keep letter h1 margin-top 0, got {named_h1_before}: {heading1_named}"
    );
    assert!(
        heading1.contains(r#"style:parent-style-name="Heading_20_1""#),
        "Heading1 must parent Writer Heading 1 so the sidebar shows it: {heading1}"
    );
    let h1_rule = odf_len_hu(
        attr(heading1, "fo:border-bottom")
            .and_then(|v| v.split_whitespace().next())
            .expect("Heading1 fo:border-bottom width"),
    );
    assert!(
        (200..350).contains(&h1_rule) && heading1.contains("solid #134e4a"),
        "Heading1 must keep letter 3px #134e4a rule, got {h1_rule}: {heading1}"
    );
    assert!(
        heading1.contains("fo:color=\"#134e4a\"")
            && heading1.contains(r#"style:use-window-font-color="false""#)
            && !heading1.contains(r#"style:use-window-font-color="true""#)
            && !heading1.contains("fo:color=\"#000080\"")
            && !heading1.contains("fo:color=\"#0000ff\"")
            && !heading1.contains("fo:color=\"#2F5496\""),
        "Heading1 type must stay letter #134e4a, not Writer navy: {heading1}"
    );
    assert!(
        heading1.contains(r#"fo:letter-spacing="-0.36pt""#)
            && !heading1.contains(r#"fo:letter-spacing="0pt""#)
            && !heading1.contains(r#"fo:letter-spacing="normal""#),
        "Heading1 must keep letter.css h1 -.02em tracking (-0.36pt at 18pt), not 0pt: {heading1}"
    );
    let heading2 = style_xml(&content, "Heading2", "paragraph");
    assert!(
        attr(heading1, "fo:line-height") == Some("120%")
            && attr(heading2, "fo:line-height") == Some("120%")
            && !heading1.contains(r#"fo:line-height="115%""#)
            && !heading2.contains(r#"fo:line-height="115%""#),
        "Heading1/2 must keep letter.css line-height:1.2, not body 115%: {heading1} {heading2}"
    );
    assert!(
        heading2.contains(r#"fo:font-size="16pt""#),
        "Heading2 must be Writer 16pt (115% of 14pt), not 14pt: {heading2}"
    );
    let h2_before = odf_len_hu(
        attr(heading2, "fo:margin-top").expect("fo:margin-top on Heading2"),
    );
    let h2_after = odf_len_hu(
        attr(heading2, "fo:margin-bottom").expect("fo:margin-bottom on Heading2"),
    );
    assert!(
        (1100..1600).contains(&h2_before),
        "Heading2 before must be letter 1.1rem (~1320 HU), not Writer 0.139in {h2_before}: {heading2}"
    );
    assert!(
        !heading2.contains(r#"fo:margin-top="0.139in""#),
        "Heading2 must not keep Writer 0.139in before: {heading2}"
    );
    assert!(
        (500..580).contains(&h2_after),
        "Heading2 after must be letter 0.45rem (~540 HU), not Writer 0.0835in {h2_after}: {heading2}"
    );
    assert!(
        !heading2.contains(r#"fo:margin-bottom="0.0835in""#),
        "Heading2 must not keep Writer 0.0835in after: {heading2}"
    );
    assert!(
        heading2.contains(r#"style:parent-style-name="Heading_20_2""#)
            && heading2.contains("fo:color=\"#134e4a\"")
            && heading2.contains(r#"style:use-window-font-color="false""#)
            && !heading2.contains("fo:color=\"#000080\""),
        "Heading2 must parent Writer Heading 2 and keep letter teal, not Writer navy: {heading2}"
    );
    let h2_rule = odf_len_hu(
        attr(heading2, "fo:border-bottom")
            .and_then(|v| v.split_whitespace().next())
            .expect("Heading2 fo:border-bottom width"),
    );
    assert!(
        (120..180).contains(&h2_rule) && heading2.contains("solid #0d9488"),
        "Heading2 must keep letter 2px #0d9488 rule, got {h2_rule}: {heading2}"
    );
    assert!(
        content.contains(r#"text:style-name="Pbody""#),
        "letter body copy must use Pbody so the page is not jammed: {content}"
    );
    let pbody = style_xml(&content, "Pbody", "paragraph");
    let body_after = odf_len_hu(
        attr(pbody, "fo:margin-bottom").expect("fo:margin-bottom on Pbody"),
    );
    assert!(
        (800..880).contains(&body_after),
        "Pbody after must be letter.css p 0.7rem (~840 HU), not Writer 0.0972in {body_after}: {pbody}"
    );
    assert!(
        !pbody.contains(r#"fo:margin-bottom="0.0972in""#),
        "Pbody must not keep Writer Text body 0.0972in after: {pbody}"
    );
    assert!(
        attr(pbody, "fo:line-height") == Some("115%"),
        "Pbody must use Writer 1.15 lines so body is not jammed: {pbody}"
    );
    assert!(
        pbody.contains("fo:color=\"#134e4a\"")
            && pbody.contains(r#"style:use-window-font-color="false""#)
            && !pbody.contains("fo:color=\"#000\"")
            && !pbody.contains("fo:color=\"#000000\"")
            && !pbody.contains("fo:color=\"#111827\""),
        "Pbody type must stay letter.html #134e4a, not UA black: {pbody}"
    );
    assert!(
        pbody.contains(r#"style:font-name="Segoe UI""#)
            && pbody.contains(r#"fo:font-family="Segoe UI""#)
            && !pbody.contains(r#"style:font-name="Liberation Sans""#),
        "Pbody must stay letter Segoe UI, not Writer Liberation Sans: {pbody}"
    );
    assert!(
        content.contains(r#"<text:p text:style-name="Pbody">6 September 2026</text:p>"#),
        "date paragraph must keep Writer body spacing: {content}"
    );
    assert!(
        content.contains("<table:table"),
        "ODT from letter.md must include a table: {content}"
    );
    assert!(
        content.contains("table:style-name="),
        "LibreOffice table must have a table:style: {content}"
    );
    assert!(
        content.contains("<table:table-column"),
        "LibreOffice table must emit table:table-column widths: {content}"
    );
    assert!(
        content.contains("style:column-width"),
        "table columns must declare style:column-width: {content}"
    );
    assert!(
        content.contains(r#"fo:border="0.5pt solid #cbd5e1""#)
            && !content.contains(r#"fo:border="0.5pt solid #000000""#),
        "LibreOffice hop grid must be 0.5pt #cbd5e1, not TableGrid black: {content}"
    );
    assert!(
        content.contains(r#"table:style-name="Tcell""#),
        "body cells must use a bordered cell style: {content}"
    );
    let tcell = style_xml(&content, "Tcell", "table-cell");
    let pad_x = cell_pad_hu(tcell, "left");
    let pad_y = cell_pad_hu(tcell, "top");
    assert!(
        tcell.contains(r#"fo:border="0.5pt solid #cbd5e1""#)
            && !tcell.contains(r#"fo:border="0.5pt solid #000000""#),
        "Tcell must be letter 0.5pt #cbd5e1, not LibreOffice black: {tcell}"
    );
    assert!(
        pad_x >= 780 && pad_y >= 480,
        "table cell padding must be letter CSS 0.65rem/0.4rem (780/480 HU), got {pad_x}×{pad_y}: {tcell}"
    );
    assert!(
        content.contains("fo:padding="),
        "cells must set fo:padding so text is not jammed on the 0.5pt grid: {content}"
    );
    assert!(
        !content.contains(r#"fo:padding="0.0382in""#),
        "LibreOffice 0.097cm default pad still jams text on a 0.5pt grid: {content}"
    );
    assert!(
        content.contains(r#"table:style-name="THcell""#),
        "header cells must use THcell so Writer paints the teal fill: {content}"
    );
    let thcell = style_xml(&content, "THcell", "table-cell");
    let th_pad_x = cell_pad_hu(thcell, "left");
    let th_pad_y = cell_pad_hu(thcell, "top");
    assert!(
        thcell.contains("fo:background-color=\"#ccfbf1\""),
        "THcell must keep letter #ccfbf1 fill, not a sparse grid: {thcell}"
    );
    assert!(
        thcell.contains(r#"fo:border="0.5pt solid #cbd5e1""#)
            && !thcell.contains(r#"fo:border="0.5pt solid #000000""#),
        "THcell must keep the 0.5pt #cbd5e1 grid, not LibreOffice black: {thcell}"
    );
    let th_rule = odf_len_hu(
        attr(thcell, "fo:border-bottom")
            .and_then(|v| v.split_whitespace().next())
            .expect("THcell fo:border-bottom width"),
    );
    assert!(
        (120..180).contains(&th_rule) && thcell.contains("solid #0d9488"),
        "THcell header rule must be letter 2px #0d9488 (~150 HU), got {th_rule}: {thcell}"
    );
    assert!(
        th_pad_x >= 780 && th_pad_y >= 480,
        "THcell padding must match Tcell letter 0.65rem/0.4rem, got {th_pad_x}×{th_pad_y}: {thcell}"
    );
    assert!(
        tcell.contains(r#"style:vertical-align="top""#)
            && thcell.contains(r#"style:vertical-align="top""#)
            && !tcell.contains(r#"style:vertical-align="middle""#)
            && !thcell.contains(r#"style:vertical-align="middle""#)
            && !tcell.contains(r#"style:vertical-align="automatic""#)
            && !thcell.contains(r#"style:vertical-align="automatic""#),
        "hop cells must stay letter vertical-align top, not Writer/UA middle: {tcell} {thcell}"
    );
    assert!(
        thcell.contains("fo:color=\"#134e4a\"")
            && thcell.contains(r#"style:use-window-font-color="false""#)
            && thcell.contains(r#"fo:font-weight="bold""#),
        "THcell text must stay letter teal bold: {thcell}"
    );
    assert!(
        !thcell.contains("transparent")
            && !thcell.contains(r#"fo:background-color="none"#)
            && !thcell.contains("fo:background-color=\"#fff\"")
            && !thcell.contains("fo:background-color=\"#ffffff\"")
            && !thcell.contains(r#"fo:border="none"#)
            && !thcell.contains(r#"fo:font-weight="400"#),
        "UA/reset THcell is a sparse grid, not WeasyPrint letter input: {thcell}"
    );
    assert!(
        !tcell.contains("fo:background-color=\"#ccfbf1\""),
        "body Tcell must not keep th fill: {tcell}"
    );
    assert!(
        tcell.contains("fo:color=\"#134e4a\"")
            && tcell.contains(r#"style:use-window-font-color="false""#)
            && !tcell.contains(r#"fo:font-weight="bold""#),
        "body Tcell type must stay letter teal, not th bold or UA black: {tcell}"
    );
    let hop_cell = table_cell_start_containing(&content, "Hop");
    let input_cell = table_cell_start_containing(&content, "Input");
    assert!(
        hop_cell.contains(r#"table:style-name="THcell""#),
        "letter.md Hop header must use THcell: {hop_cell}"
    );
    let hop_p = paragraph_containing(&content, "Hop");
    assert!(
        hop_p.contains(r#"text:style-name="Pth""#),
        "Hop header text must use Pth so Writer paints teal bold, not missing THcell text-properties: {hop_p}"
    );
    let pth = style_xml(&content, "Pth", "paragraph");
    assert!(
        pth.contains("fo:color=\"#134e4a\"")
            && pth.contains(r#"style:use-window-font-color="false""#)
            && pth.contains(r#"fo:font-weight="bold""#),
        "Pth must keep letter teal bold: {pth}"
    );
    assert!(
        pth.contains(r#"fo:font-size="11.4pt""#)
            && !pth.contains(r#"fo:font-size="10pt""#),
        "Pth must be letter 0.95rem (11.4pt), not body 10pt: {pth}"
    );
    let input_p = paragraph_containing(&content, "Input");
    assert!(
        input_p.contains(r#"text:style-name="Pcell""#),
        "letter.md Input body must use Pcell so Writer paints 0.95rem, not missing Tcell text-properties: {input_p}"
    );
    let pcell = style_xml(&content, "Pcell", "paragraph");
    assert!(
        pcell.contains(r#"fo:font-size="11.4pt""#)
            && pcell.contains("fo:color=\"#134e4a\"")
            && pcell.contains(r#"style:use-window-font-color="false""#)
            && !pcell.contains(r#"fo:font-weight="bold""#)
            && !pcell.contains(r#"fo:font-size="10pt""#),
        "Pcell must be letter 0.95rem (11.4pt) teal, not body 10pt or th bold: {pcell}"
    );
    assert!(
        tcell.contains(r#"fo:font-size="11.4pt""#)
            && thcell.contains(r#"fo:font-size="11.4pt""#),
        "Tcell/THcell must keep letter 0.95rem (11.4pt): {tcell} {thcell}"
    );
    assert!(
        input_cell.contains(r#"table:style-name="Tcell""#),
        "letter.md Input body must stay Tcell: {input_cell}"
    );
    let table1 = style_xml(&content, "Table1", "table");
    let table_before = odf_len_hu(
        attr(table1, "fo:margin-top").expect("fo:margin-top on Table1"),
    );
    let table_after = odf_len_hu(
        attr(table1, "fo:margin-bottom").expect("fo:margin-bottom on Table1"),
    );
    assert!(
        (1000..1600).contains(&table_before) && (1000..1600).contains(&table_after),
        "LibreOffice table must keep letter 1rem (~1200 HU), got {table_before}/{table_after}: {table1}"
    );
    assert!(
        table1.contains(r#"table:border-model="collapsing""#)
            && !table1.contains(r#"table:border-model="separating""#)
            && !table1.contains("fo:border-spacing="),
        "hop table must stay letter.html border-collapse:collapse, not Writer separating 2px: {table1}"
    );
    assert!(
        content.contains("Hop") && content.contains("letter.md"),
        "ODT table must keep the hop rows: {content}"
    );
    assert!(
        content.contains("<draw:image"),
        "ODT from letter.md must include draw:image: {content}"
    );
    assert!(
        content.contains("Pictures/"),
        "draw:image must href Pictures/: {content}"
    );
    assert!(
        content.contains("Harbor mark"),
        "Harbor mark alt must appear on the frame: {content}"
    );
    assert!(
        !content.contains("svg:width=\"0.1666in\""),
        "Harbor mark must not be the 16px/12pt box: {content}"
    );
    assert!(
        !content.contains("svg:height=\"0.1666in\""),
        "Harbor mark must not be the 16px/12pt box: {content}"
    );

    let (width, height) = frame_size_hu(&content);
    assert!(
        width >= MIN_IMAGE_EDGE_HU,
        "draw:frame svg:width must be at least 24pt ({MIN_IMAGE_EDGE_HU} HU), got {width}: {content}"
    );
    assert!(
        height >= MIN_IMAGE_EDGE_HU,
        "draw:frame svg:height must be at least 24pt ({MIN_IMAGE_EDGE_HU} HU), got {height}: {content}"
    );

    let mark_p = harbor_paragraph(&content);
    assert!(
        mark_p.contains(r#"draw:style-name="Ginline""#)
            && mark_p.contains(r#"draw:z-index="0""#)
            && mark_p.contains(r#"text:anchor-type="as-char""#),
        "Harbor draw:frame must use Writer Ginline + z-index so the PNG is visible: {mark_p}"
    );
    let ginline = style_xml(&content, "Ginline", "graphic");
    assert!(
        ginline.contains(r#"draw:fill="none""#)
            && ginline.contains(r#"draw:image-opacity="100%""#)
            && ginline.contains(r#"style:background-transparency="100%""#)
            && ginline.contains(r#"fo:border="none""#)
            && ginline.contains(r#"draw:color-mode="standard""#)
            && ginline.contains(r#"style:vertical-rel="baseline""#),
        "Ginline must not fill over the PNG: {ginline}"
    );
    assert!(
        mark_p.contains(r#"text:style-name="Pimage""#),
        "Harbor mark paragraph must use Pimage so the page is not sparse under the image: {mark_p}"
    );
    let pimage = style_xml(&content, "Pimage", "paragraph");
    let before = odf_len_hu(attr(pimage, "fo:margin-top").expect("fo:margin-top on Pimage"));
    let after = odf_len_hu(attr(pimage, "fo:margin-bottom").expect("fo:margin-bottom on Pimage"));
    assert!(
        (500..1000).contains(&before),
        "gap before Harbor mark must be letter 0.6rem (~720 HU), not jammed under H2 {before}: {pimage}"
    );
    assert!(
        after >= 840,
        "gap after Harbor mark must be at least letter 0.7rem (840 HU), got {after}: {pimage}"
    );
    assert!(
        after < 840 + 400,
        "gap after Harbor mark {after} must not leave the page sparse under the image: {pimage}"
    );
    assert!(
        before < after,
        "img 0.6rem before must stay tighter than collapsed 0.7rem after, got {before} vs {after}: {pimage}"
    );

    assert!(
        content.contains(r#"text:style-name="Quote""#),
        "letter blockquote must use Quote: {content}"
    );
    let quote = style_xml(&content, "Quote", "paragraph");
    let quote_before = odf_len_hu(attr(quote, "fo:margin-top").expect("Quote fo:margin-top"));
    let quote_after = odf_len_hu(attr(quote, "fo:margin-bottom").expect("Quote fo:margin-bottom"));
    let quote_pad_x = odf_len_hu(attr(quote, "fo:padding-left").expect("Quote fo:padding-left"));
    assert!(
        (400..900).contains(&quote_before),
        "Quote before must be letter 0.5rem (~600 HU), not jammed/sparse {quote_before}: {quote}"
    );
    assert!(
        (1000..1600).contains(&quote_after),
        "Quote after must be letter 1rem (~1200 HU), not jammed/sparse {quote_after}: {quote}"
    );
    assert!(
        quote_pad_x >= 1000,
        "Quote pad-x must be letter 1rem (~1200 HU), got {quote_pad_x}: {quote}"
    );
    let quote_border = attr(quote, "fo:border-left").expect("Quote fo:border-left");
    let quote_bar = odf_len_hu(
        quote_border
            .split_whitespace()
            .next()
            .expect("Quote fo:border-left width"),
    );
    assert!(
        (250..450).contains(&quote_bar),
        "Quote bar must be letter 4px (~300 HU), not 0.02in, got {quote_bar}: {quote}"
    );
    assert!(
        quote_border.ends_with("solid #134e4a"),
        "Quote fo:border-left must match letter.html 4px #134e4a, not Writer gray: {quote}"
    );
    assert!(
        quote.contains(r#"fo:border-right="none""#)
            && quote.contains(r#"fo:border-top="none""#)
            && quote.contains(r#"fo:border-bottom="none""#),
        "Quote must keep other sides none so Writer paints a left bar, not a box: {quote}"
    );
    assert!(
        quote.contains("fo:color=\"#134e4a\""),
        "Quote text must stay letter.html #134e4a like the 4px bar: {quote}"
    );
    assert!(
        !quote.contains(r#"fo:border-left="none"#)
            && !quote.contains(r#"fo:border-left="0in"#)
            && !quote.contains(r#"fo:border-left="0.02in"#)
            && !quote.contains(r#"fo:border-left="0.0492in"#)
            && !quote.contains("solid #ccc")
            && !quote.contains("solid #999")
            && !quote.contains("solid #808080")
            && !quote.contains("solid #000"),
        "UA/reset/0.02in/gray quote bar is not WeasyPrint letter input: {quote}"
    );
    assert!(
        content.contains(r#"text:style-name="CodeBlock""#),
        "letter code_block must use CodeBlock: {content}"
    );
    let code = style_xml(&content, "CodeBlock", "paragraph");
    let code_before = odf_len_hu(attr(code, "fo:margin-top").expect("CodeBlock fo:margin-top"));
    let code_after = odf_len_hu(attr(code, "fo:margin-bottom").expect("CodeBlock fo:margin-bottom"));
    let code_pad_x = odf_len_hu(attr(code, "fo:padding-left").expect("CodeBlock fo:padding-left"));
    let code_pad_y = odf_len_hu(attr(code, "fo:padding-top").expect("CodeBlock fo:padding-top"));
    assert!(
        (500..1000).contains(&code_before),
        "CodeBlock before must be letter 0.6rem (~720 HU), not jammed {code_before}: {code}"
    );
    assert!(
        (1000..1600).contains(&code_after),
        "CodeBlock after must be letter 1rem (~1200 HU), not jammed {code_after}: {code}"
    );
    assert!(
        code_pad_x >= 1000 && code_pad_y >= 700,
        "CodeBlock pad must be letter 1rem/0.75rem, got {code_pad_x}×{code_pad_y}: {code}"
    );
    assert!(
        !code.contains(r#"fo:padding="0.1in""#),
        "CodeBlock must not keep the jammed 0.1in pad: {code}"
    );

    let dock_p = paragraph_containing(&content, "Receiving dock is clear");
    let bay_p = paragraph_containing(&content, "Bay 4");
    let capsule_p = paragraph_containing(&content, "Capsule hashes travel with the files");
    let convert_p = paragraph_containing(&content, "Run Convert");
    let last_task_p = paragraph_containing(&content, "Replay the convert instead of hoping");
    let last_num_p = paragraph_containing(&content, "Compare the capsule");
    assert!(
        dock_p.contains(r#"text:style-name="PlistNested""#),
        "parent of nested Bay 4 must use PlistNested 0.25rem, not consecutive Plist 0.35rem: {dock_p}"
    );
    assert!(
        capsule_p.contains(r#"text:style-name="Plist""#),
        "consecutive list items must use Plist, not Pbody: {capsule_p}"
    );
    assert!(
        convert_p.contains(r#"text:style-name="PlistNested""#),
        "parent of nested PDF/HTML must use PlistNested 0.25rem: {convert_p}"
    );
    assert!(
        bay_p.contains(r#"text:style-name="Plist""#),
        "nested list items must use Plist: {bay_p}"
    );
    assert!(
        last_task_p.contains(r#"text:style-name="PlistLast""#),
        "last task item must use PlistLast so the list is not jammed into the next block: {last_task_p}"
    );
    assert!(
        last_num_p.contains(r#"text:style-name="PlistLast""#),
        "last decimal item must use PlistLast: {last_num_p}"
    );
    let hash_p = paragraph_containing(&content, "Hash the input");
    assert!(
        hash_p.contains(r#"text:style-name="Plist""#),
        "consecutive decimal items must stay Plist 0.35rem, not PlistLast 1rem: {hash_p}"
    );
    let plist = style_xml(&content, "Plist", "paragraph");
    let plist_nested = style_xml(&content, "PlistNested", "paragraph");
    let plist_last = style_xml(&content, "PlistLast", "paragraph");
    let item_after = odf_len_hu(attr(plist, "fo:margin-bottom").expect("Plist fo:margin-bottom"));
    let nested_after = odf_len_hu(
        attr(plist_nested, "fo:margin-bottom").expect("PlistNested fo:margin-bottom"),
    );
    let list_after = odf_len_hu(
        attr(plist_last, "fo:margin-bottom").expect("PlistLast fo:margin-bottom"),
    );
    assert!(
        (300..600).contains(&item_after),
        "Plist after must be letter 0.35rem (~420 HU), not jammed/sparse {item_after}: {plist}"
    );
    assert!(
        (200..400).contains(&nested_after) && nested_after < item_after,
        "PlistNested after must be letter 0.25rem (~300 HU), not consecutive 0.35rem {nested_after}: {plist_nested}"
    );
    assert!(
        (1000..1600).contains(&list_after) && list_after > item_after,
        "PlistLast after must be letter 1rem (~1200 HU), larger than consecutive 0.35rem, got {list_after} vs {item_after}: {plist_last}"
    );
    assert!(
        attr(plist, "fo:line-height") == Some("115%"),
        "Plist must keep Writer 1.15 lines: {plist}"
    );
    assert!(
        attr(plist_nested, "fo:line-height") == Some("115%"),
        "PlistNested must keep Writer 1.15 lines: {plist_nested}"
    );
    assert!(
        attr(plist_last, "fo:line-height") == Some("115%"),
        "PlistLast must keep Writer 1.15 lines: {plist_last}"
    );
    assert!(
        plist.contains("fo:color=\"#134e4a\"")
            && plist_nested.contains("fo:color=\"#134e4a\"")
            && plist_last.contains("fo:color=\"#134e4a\""),
        "list body type must stay letter.html #134e4a, not UA black: {plist} {plist_nested} {plist_last}"
    );
    let lbullet = list_style_xml(&content, "Lbullet");
    let lnumber = list_style_xml(&content, "Lnumber");
    let ltask = list_style_xml(&content, "Ltask");
    for (name, style) in [("Lbullet", lbullet), ("Lnumber", lnumber), ("Ltask", ltask)] {
        assert!(
            style.contains(r#"text:list-level-position-and-space-mode="label-alignment""#),
            "{name} must use Writer label-alignment: {style}"
        );
        let left = list_level_left_hu(style, 1);
        let nested = list_level_left_hu(style, 2);
        let hang = odf_len_hu(attr(style, "fo:text-indent").expect("fo:text-indent hanging"));
        assert!(
            (1400..1600).contains(&left),
            "{name} level 1 must be letter 1.25rem (~1500 HU), not Writer 0.5in, got {left}: {style}"
        );
        assert!(
            (2800..3200).contains(&nested),
            "{name} level 2 must be letter 2.5rem (~3000 HU), not Writer 0.75in, got {nested}: {style}"
        );
        assert!(
            hang <= -1100 && hang > -1300,
            "{name} hanging must stay inside the 1.25rem gutter (~-1200 HU), not Writer -0.25in, got {hang}: {style}"
        );
        assert!(
            !style.contains(r#"fo:margin-left="0.5in""#)
                && !style.contains(r#"fo:margin-left="0.75in""#)
                && !style.contains(r#"fo:text-indent="-0.25in""#),
            "{name} must not keep Writer 0.5in/0.75in/-0.25in: {style}"
        );
        assert!(
            !style.contains("text:space-before"),
            "{name} must not keep the old space-before indent: {style}"
        );
    }

    assert!(
        content.contains(r#"<text:list text:style-name="Ltask">"#),
        "letter.md - [x]/[ ] must emit Writer Ltask, not a disc: {content}"
    );
    let ltask = list_style_xml(&content, "Ltask");
    let ltask_done = list_style_xml(&content, "LtaskDone");
    assert!(
        ltask.contains("text:bullet-char=\"\u{2610}\""),
        "unchecked To-Do must be Writer ballot box, not a missing [ ]: {ltask}"
    );
    assert!(
        ltask_done.contains("text:bullet-char=\"\u{2611}\""),
        "checked To-Do must be Writer ballot-with-check, not a missing [x]: {ltask_done}"
    );
    assert!(
        ltask.contains(r#"text:style-name="TtaskBullet""#)
            && ltask.contains(r#"text:list-level-position-and-space-mode="label-alignment""#),
        "Ltask must keep Writer label-alignment + TtaskBullet: {ltask}"
    );
    let ttask = style_xml(&content, "TtaskBullet", "text");
    assert!(
        ttask.contains("fo:color=\"#134e4a\""),
        "task boxes must be letter teal: {ttask}"
    );
    let dock_item = list_item_start_containing(&content, "Receiving dock is clear");
    let capsule_item = list_item_start_containing(&content, "Capsule hashes travel with the files");
    let replay_item = list_item_start_containing(&content, "Replay the convert instead of hoping");
    assert!(
        dock_item.contains(r#"text:style-override="LtaskDone""#),
        "letter.md - [x] Receiving dock must override to LtaskDone: {dock_item}"
    );
    assert!(
        capsule_item.contains(r#"text:style-override="LtaskDone""#),
        "letter.md - [x] Capsule hashes must override to LtaskDone: {capsule_item}"
    );
    assert!(
        !replay_item.contains("style-override"),
        "letter.md - [ ] Replay must stay unchecked Ltask: {replay_item}"
    );
    assert!(
        !dock_p.contains("[x]") && !last_task_p.contains("[ ]") && !last_task_p.contains("[x]"),
        "task item text must not keep missing-box ASCII markers: {dock_p} {last_task_p}"
    );
    assert!(
        !content.contains("[x] Receiving") && !content.contains("[ ] Replay"),
        "letter.md tasks must look like Writer boxes, not missing [x]/[ ]: {content}"
    );
    let bay_item = list_item_start_containing(&content, "Bay 4");
    assert!(
        !bay_item.contains("LtaskDone") && !bay_item.contains("style-override"),
        "nested Bay 4 under a task stays a bullet, not a checkbox: {bay_item}"
    );

    assert!(
        content.contains(r#"text:style-name="HorizontalLine""#),
        "letter thematic break must emit Writer HorizontalLine: {content}"
    );
    let hr = style_xml(&content, "HorizontalLine", "paragraph");
    let hr_border = odf_len_hu(
        attr(hr, "fo:border-top")
            .and_then(|v| v.split_whitespace().next())
            .expect("HorizontalLine fo:border-top width"),
    );
    let hr_before = odf_len_hu(attr(hr, "fo:margin-top").expect("HorizontalLine fo:margin-top"));
    let hr_after = odf_len_hu(
        attr(hr, "fo:margin-bottom").expect("HorizontalLine fo:margin-bottom"),
    );
    assert!(
        (200..350).contains(&hr_border),
        "HorizontalLine must be letter 3px (~225 HU), not missing 0.02in, got {hr_border}: {hr}"
    );
    assert!(
        (1100..1600).contains(&hr_before) && (1100..1600).contains(&hr_after),
        "HorizontalLine must be letter 1.1rem (~1320 HU), not 0.15in, got {hr_before}/{hr_after}: {hr}"
    );
    assert!(
        !hr.contains(r#"fo:border-bottom="0.02in solid"#) && !hr.contains(r#"fo:margin-top="0.15in""#),
        "must not keep the missing 0.02in/0.15in rule: {hr}"
    );

    let link_p = paragraph_containing(&content, "DocAgent");
    assert!(
        link_p.contains(r#"<text:a text:style-name="Tlink""#)
            && link_p.contains(r#"xlink:type="simple""#)
            && link_p.contains(r#"xlink:href="https://github.com/kevin9327/docagent""#)
            && link_p.contains("DocAgent</text:a>"),
        "letter link must be a visible text:a, not missing body text: {link_p}"
    );
    let tlink = style_xml(&content, "Tlink", "text");
    assert!(
        tlink.contains("fo:color=\"#0d9488\"")
            && tlink.contains(r#"style:parent-style-name="Internet_20_Link""#)
            && tlink.contains(r#"style:use-window-font-color="false""#),
        "Tlink must be letter teal Internet Link, not Writer navy: {tlink}"
    );
    assert!(
        tlink.contains(r#"style:text-underline-style="solid""#),
        "Tlink must underline like Writer Internet Link: {tlink}"
    );
    assert!(
        tlink.contains(r#"style:text-underline-width="auto""#)
            && tlink.contains(r#"style:text-underline-color="font-color""#),
        "Tlink must keep Writer underline width/color: {tlink}"
    );
    let internet = office_style_xml(&styles, "Internet_20_Link");
    let visited = office_style_xml(&styles, "Visited_20_Internet_20_Link");
    assert!(
        styles.contains(r#"style:display-name="Internet Link""#)
            && styles.contains(r#"style:display-name="Visited Internet Link""#),
        "styles.xml must include Writer Internet Link / Visited: {styles}"
    );
    assert!(
        internet.contains("fo:color=\"#0d9488\"")
            && internet.contains(r#"style:use-window-font-color="false""#)
            && internet.contains(r#"style:text-underline-style="solid""#)
            && !internet.contains("fo:color=\"#000080\"")
            && !internet.contains("fo:color=\"#0000ff\""),
        "Writer Internet Link must stay letter #0d9488, not navy: {internet}"
    );
    assert!(
        visited.contains("fo:color=\"#0d9488\"")
            && visited.contains(r#"style:use-window-font-color="false""#)
            && visited.contains(r#"style:text-underline-style="solid""#)
            && !visited.contains("fo:color=\"#800000\"")
            && !visited.contains("fo:color=\"#800080\""),
        "Visited Internet Link must stay letter #0d9488, not Writer maroon: {visited}"
    );

    let flow_p = paragraph_containing(&content, "shipment");
    assert!(
        flow_p.contains(r#"<text:span text:style-name="Tcode">prove</text:span>"#),
        "letter inline code must be Tcode, not plain text: {flow_p}"
    );
    assert!(
        flow_p.contains(r#"<text:span text:style-name="Tmark">three hashes</text:span>"#),
        "letter highlight must be Tmark so Writer shows amber: {flow_p}"
    );
    assert!(
        flow_p.contains(r#"<text:span text:style-name="Tsuper">A</text:span>"#),
        "letter PDF/^A^ must be Tsuper so Writer raises the A, not missing: {flow_p}"
    );
    assert!(
        flow_p.contains(r#"<text:span text:style-name="Tsub">2</text:span>"#),
        "letter H~2~O must be Tsub so Writer lowers the 2, not missing: {flow_p}"
    );
    let tcode = style_xml(&content, "Tcode", "text");
    let tcode_pad_x = odf_len_hu(
        attr(tcode, "fo:padding-left").expect("fo:padding-left on Tcode"),
    );
    let tcode_pad_y = odf_len_hu(
        attr(tcode, "fo:padding")
            .or_else(|| attr(tcode, "fo:padding-top"))
            .expect("fo:padding on Tcode"),
    );
    assert!(
        tcode.contains("fo:background-color=\"#ccfbf1\""),
        "Tcode background must be letter #ccfbf1, not a bare mono span: {tcode}"
    );
    assert!(
        tcode.contains("fo:color=\"#134e4a\"")
            && tcode.contains(r#"style:font-name="Consolas""#)
            && tcode.contains(r#"fo:font-family="Consolas""#)
            && tcode.contains(r#"fo:font-size="92%""#),
        "Tcode must keep letter teal Consolas .92em: {tcode}"
    );
    assert!(
        (300..450).contains(&tcode_pad_x) && (80..150).contains(&tcode_pad_y),
        "Tcode pad must be letter .1em/.35em (~100×350 HU), got {tcode_pad_y}×{tcode_pad_x}: {tcode}"
    );
    assert!(
        !tcode.contains(r#"fo:padding="0in""#)
            && !tcode.contains("transparent")
            && !tcode.contains(r#"fo:background-color="none"#),
        "Tcode must not reset the chip: {tcode}"
    );
    let tmark = style_xml(&content, "Tmark", "text");
    let tmark_pad_x = odf_len_hu(
        attr(tmark, "fo:padding-left").expect("fo:padding-left on Tmark"),
    );
    let tmark_pad_y = odf_len_hu(
        attr(tmark, "fo:padding")
            .or_else(|| attr(tmark, "fo:padding-top"))
            .expect("fo:padding on Tmark"),
    );
    assert!(
        tmark.contains("fo:background-color=\"#fde68a\""),
        "Tmark must be letter amber so highlights are visible: {tmark}"
    );
    assert!(
        tmark.contains("fo:color=\"#134e4a\"")
            && tmark.contains(r#"style:use-window-font-color="false""#)
            && !tmark.contains(r#"style:use-window-font-color="true""#),
        "Tmark type must stay letter.html #134e4a, not UA black on yellow: {tmark}"
    );
    assert!(
        tmark.contains(r#"style:parent-style-name="Mark""#)
            && tmark.contains(r#"style:font-name="Segoe UI""#),
        "Tmark must parent Writer Mark + Segoe UI so UA Highlight is not black-on-amber: {tmark}"
    );
    let mark_named = office_style_xml(&styles, "Mark");
    assert!(
        styles.contains(r#"style:display-name="Mark""#)
            && mark_named.contains("fo:background-color=\"#fde68a\"")
            && mark_named.contains("fo:color=\"#134e4a\"")
            && mark_named.contains(r#"style:use-window-font-color="false""#)
            && !mark_named.contains("fo:color=\"#000000\""),
        "Writer Mark must stay letter teal on amber, not UA black: {mark_named}"
    );
    assert!(
        (150..280).contains(&tmark_pad_x) && (30..90).contains(&tmark_pad_y),
        "Tmark pad must be letter .05em/.2em (~50×200 HU), got {tmark_pad_y}×{tmark_pad_x}: {tmark}"
    );
    assert!(
        !tmark.contains("transparent")
            && !tmark.contains("#ffff00")
            && !tmark.contains("fo:color=\"#000\"")
            && !tmark.contains("fo:color=\"#000000\"")
            && !tmark.contains(r#"fo:padding="0in""#),
        "Tmark must not drop to UA yellow/black or a tight wash: {tmark}"
    );
    assert!(
        content.contains(r#"style:name="Ltask""#)
            && content.contains("text:bullet-char=\"\u{2610}\"")
            && content.contains("text:bullet-char=\"\u{2611}\""),
        "Tcode/Tmark must keep Writer task boxes: {content}"
    );

    let guess_p = paragraph_containing(&content, "guess");
    assert!(
        guess_p.contains(r#"<text:span text:style-name="Tstrike">guess</text:span>"#),
        "letter ~~guess~~ must be Tstrike so Writer paints the line, not missing: {guess_p}"
    );
    let confirm_p = paragraph_containing(&content, "09:00 local");
    assert!(
        confirm_p.contains(r#"<text:span text:style-name="Tunder">09:00 local</text:span>"#),
        "letter __09:00 local__ must be Tunder so Writer paints the rule, not missing: {confirm_p}"
    );
    let tstrike = style_xml(&content, "Tstrike", "text");
    assert!(
        tstrike.contains(r#"style:text-line-through-style="solid""#)
            && tstrike.contains(r#"style:text-line-through-type="single""#)
            && tstrike.contains(r#"style:text-line-through-width="auto""#)
            && tstrike.contains(r#"style:text-line-through-color="font-color""#),
        "Tstrike must be Writer Single line-through, not a missing rule: {tstrike}"
    );
    assert!(
        tstrike.contains("fo:color=\"#134e4a\"")
            && tstrike.contains(r#"style:use-window-font-color="false""#),
        "Tstrike type must stay letter.html #134e4a, not UA black: {tstrike}"
    );
    assert!(
        !tstrike.contains(r#"style:text-line-through-style="none""#)
            && !tstrike.contains(r#"style:text-line-through-type="none""#),
        "Tstrike must not reset to a missing line: {tstrike}"
    );
    let tunder = style_xml(&content, "Tunder", "text");
    assert!(
        tunder.contains(r#"style:text-underline-style="solid""#)
            && tunder.contains(r#"style:text-underline-width="auto""#)
            && tunder.contains(r#"style:text-underline-color="font-color""#),
        "Tunder must be Writer Single underline, not a missing rule: {tunder}"
    );
    assert!(
        !tunder.contains(r#"style:text-underline-style="none""#),
        "Tunder must not reset to a missing rule: {tunder}"
    );

    let tsuper = style_xml(&content, "Tsuper", "text");
    assert!(
        tsuper.contains(r#"style:text-position="super 58%""#),
        "Tsuper must be Writer Automatic super 58%, not missing: {tsuper}"
    );
    assert!(
        !tsuper.contains(r#"style:text-position="none""#)
            && !tsuper.contains(r#"style:text-position="0%"#)
            && !tsuper.contains("100%")
            && !tsuper.contains(r#"style:text-position="sub"#),
        "Tsuper must not reset to baseline or subscript: {tsuper}"
    );
    let tsub = style_xml(&content, "Tsub", "text");
    assert!(
        tsub.contains(r#"style:text-position="sub 58%""#),
        "Tsub must be Writer Automatic sub 58%, not missing: {tsub}"
    );
    assert!(
        !tsub.contains(r#"style:text-position="none""#)
            && !tsub.contains(r#"style:text-position="0%"#)
            && !tsub.contains("100%")
            && !tsub.contains(r#"style:text-position="super"#),
        "Tsub must not reset to baseline or superscript: {tsub}"
    );
    assert!(
        bay_p.contains(r#"<text:span text:style-name="Tsub">2</text:span>"#),
        "nested Bay 4 H~2~O must keep Tsub: {bay_p}"
    );
    assert!(
        link_p.contains(r#"<text:span text:style-name="Tsuper">A</text:span>"#)
            && link_p.contains(r#"<text:span text:style-name="Tsub">2</text:span>"#),
        "closing PDF/^A^ and H~2~O must stay Tsuper/Tsub: {link_p}"
    );

    let mut names = Vec::new();
    for i in 0..zip.len() {
        let name = zip.by_index(i).unwrap().name().replace('\\', "/");
        if name.starts_with("Pictures/") && !name.ends_with('/') {
            names.push(name);
        }
    }
    assert!(
        !names.is_empty(),
        "ODT from letter.md must store an image under Pictures/"
    );
    for name in names {
        let mut stored = Vec::new();
        zip.by_name(&name)
            .unwrap()
            .read_to_end(&mut stored)
            .unwrap();
        assert!(!stored.is_empty(), "{name} must have bytes");
        assert!(
            stored.starts_with(PNG_MAGIC),
            "{name} must be the Harbor PNG"
        );
    }
}

#[test]
fn letter_md_odt_super_and_sub_like_writer() {
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

    let odt = docagent_odt::write(&doc).expect("odt write");
    let mut zip = ZipArchive::new(Cursor::new(odt)).expect("odt zip");
    let mut content = String::new();
    zip.by_name("content.xml")
        .expect("content.xml")
        .read_to_string(&mut content)
        .unwrap();

    let flow_p = paragraph_containing(&content, "shipment");
    assert!(
        flow_p.contains(r#"<text:span text:style-name="Tsuper">A</text:span>"#),
        "letter PDF/^A^ must be Tsuper so Writer raises the A, not missing: {flow_p}"
    );
    assert!(
        flow_p.contains(r#"<text:span text:style-name="Tsub">2</text:span>"#),
        "letter H~2~O must be Tsub so Writer lowers the 2, not missing: {flow_p}"
    );
    let tsuper = style_xml(&content, "Tsuper", "text");
    assert!(
        tsuper.contains(r#"style:text-position="super 58%""#),
        "Tsuper must be Writer Automatic super 58%: {tsuper}"
    );
    let tsub = style_xml(&content, "Tsub", "text");
    assert!(
        tsub.contains(r#"style:text-position="sub 58%""#),
        "Tsub must be Writer Automatic sub 58%: {tsub}"
    );
    assert!(
        !tsuper.contains("100%") && !tsub.contains("100%"),
        "super/sub must not stay full-size baseline: {tsuper} {tsub}"
    );

    let guess_p = paragraph_containing(&content, "guess");
    assert!(
        guess_p.contains(r#"<text:span text:style-name="Tstrike">guess</text:span>"#),
        "strike must not regress: {guess_p}"
    );
    let tstrike = style_xml(&content, "Tstrike", "text");
    assert!(
        tstrike.contains(r#"style:text-line-through-style="solid""#)
            && tstrike.contains(r#"style:text-line-through-type="single""#),
        "Tstrike must stay Writer Single line-through: {tstrike}"
    );
    let thcell = style_xml(&content, "THcell", "table-cell");
    assert!(
        thcell.contains("fo:background-color=\"#ccfbf1\"")
            && thcell.contains("solid #0d9488")
            && thcell.contains(r#"fo:font-weight="bold""#),
        "THcell teal fill must not regress: {thcell}"
    );
    assert!(
        content.contains(r#"<text:list text:style-name="Ltask">"#)
            && content.contains("text:bullet-char=\"\u{2610}\"")
            && content.contains("text:bullet-char=\"\u{2611}\""),
        "Writer task boxes must not regress: {content}"
    );
    let (width, height) = frame_size_hu(&content);
    assert!(
        width >= MIN_IMAGE_EDGE_HU && height >= MIN_IMAGE_EDGE_HU,
        "draw:frame must keep the 24pt floor, got {width}×{height}: {content}"
    );
    assert!(
        content.contains("<draw:image") && !content.contains("svg:width=\"0.1666in\""),
        "24pt draw:image must not regress to the 16px box: {content}"
    );
    let quote = style_xml(&content, "Quote", "paragraph");
    let quote_border = attr(quote, "fo:border-left").expect("Quote fo:border-left");
    let quote_bar = odf_len_hu(
        quote_border
            .split_whitespace()
            .next()
            .expect("Quote fo:border-left width"),
    );
    assert!(
        (250..450).contains(&quote_bar) && quote_border.ends_with("solid #134e4a"),
        "quote bar must not regress from letter.html 4px #134e4a, got {quote_bar}: {quote}"
    );
}

#[test]
fn letter_md_odt_quote_bar_like_writer() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("> Replay is evidence. Same fonts, same bytes."),
        "letter.md must keep the blockquote Writer paints with a 4px left bar"
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

    let odt = docagent_odt::write(&doc).expect("odt write");
    let mut zip = ZipArchive::new(Cursor::new(odt)).expect("odt zip");
    let mut content = String::new();
    zip.by_name("content.xml")
        .expect("content.xml")
        .read_to_string(&mut content)
        .unwrap();

    assert!(
        content.contains(r#"text:style-name="Quote""#)
            && content.contains("Replay is evidence. Same fonts, same bytes."),
        "letter blockquote must use Quote: {content}"
    );
    let quote = style_xml(&content, "Quote", "paragraph");
    let quote_border = attr(quote, "fo:border-left").expect("Quote fo:border-left");
    let quote_bar = odf_len_hu(
        quote_border
            .split_whitespace()
            .next()
            .expect("Quote fo:border-left width"),
    );
    assert!(
        (250..450).contains(&quote_bar),
        "Quote bar must be letter.html 4px (~300 HU), not 0.02in, got {quote_bar}: {quote}"
    );
    assert!(
        quote_border.ends_with("solid #134e4a"),
        "Quote fo:border-left must match letter.html 4px #134e4a, not Writer gray: {quote}"
    );
    assert!(
        quote.contains(r#"fo:border-right="none""#)
            && quote.contains(r#"fo:border-top="none""#)
            && quote.contains(r#"fo:border-bottom="none""#),
        "Quote must keep other sides none so Writer paints a left bar, not a box: {quote}"
    );
    assert!(
        quote.contains("fo:color=\"#134e4a\""),
        "Quote text must stay letter.html #134e4a: {quote}"
    );
    assert!(
        !quote.contains(r#"fo:border-left="none"#)
            && !quote.contains(r#"fo:border-left="0in"#)
            && !quote.contains(r#"fo:border-left="0.02in"#)
            && !quote.contains(r#"fo:border-left="0.0492in"#)
            && !quote.contains("solid #ccc")
            && !quote.contains("solid #999")
            && !quote.contains("solid #808080")
            && !quote.contains("solid #000"),
        "UA/reset/0.02in/gray quote bar is not WeasyPrint letter input: {quote}"
    );

    let flow_p = paragraph_containing(&content, "shipment");
    assert!(
        flow_p.contains(r#"<text:span text:style-name="Tsuper">A</text:span>"#)
            && flow_p.contains(r#"<text:span text:style-name="Tsub">2</text:span>"#),
        "quote bar must keep Tsuper/Tsub: {flow_p}"
    );
    let tsuper = style_xml(&content, "Tsuper", "text");
    assert!(
        tsuper.contains(r#"style:text-position="super 58%""#),
        "Tsuper must not regress: {tsuper}"
    );
    let tsub = style_xml(&content, "Tsub", "text");
    assert!(
        tsub.contains(r#"style:text-position="sub 58%""#),
        "Tsub must not regress: {tsub}"
    );
    let thcell = style_xml(&content, "THcell", "table-cell");
    assert!(
        thcell.contains("fo:background-color=\"#ccfbf1\"")
            && thcell.contains("solid #0d9488")
            && thcell.contains(r#"fo:font-weight="bold""#),
        "THcell teal fill must not regress: {thcell}"
    );
    assert!(
        content.contains(r#"<text:list text:style-name="Ltask">"#)
            && content.contains("text:bullet-char=\"\u{2610}\"")
            && content.contains("text:bullet-char=\"\u{2611}\""),
        "Writer task boxes must not regress: {content}"
    );
    let (width, height) = frame_size_hu(&content);
    assert!(
        width >= MIN_IMAGE_EDGE_HU && height >= MIN_IMAGE_EDGE_HU,
        "draw:frame must keep the 24pt floor, got {width}×{height}: {content}"
    );
    assert!(
        content.contains("<draw:image") && !content.contains("svg:width=\"0.1666in\""),
        "24pt draw:image must not regress to the 16px box: {content}"
    );
}

#[test]
fn letter_md_odt_body_color_like_writer() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("6 September 2026") && text.contains("operations desk"),
        "letter.md must keep body copy Writer paints as letter teal"
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
    let odt = docagent_odt::write(&doc).expect("odt write");
    assert!(docagent_odt::sniff(&odt), "written bytes must sniff as ODT");
    let mut zip = ZipArchive::new(Cursor::new(odt)).expect("odt zip");
    let mut content = String::new();
    zip.by_name("content.xml")
        .expect("content.xml")
        .read_to_string(&mut content)
        .unwrap();
    let mut styles = String::new();
    zip.by_name("styles.xml")
        .expect("styles.xml")
        .read_to_string(&mut styles)
        .unwrap();

    assert!(
        content.contains(r#"<text:p text:style-name="Pbody">6 September 2026</text:p>"#),
        "letter body copy must use Pbody: {content}"
    );
    let pbody = style_xml(&content, "Pbody", "paragraph");
    assert!(
        pbody.contains("fo:color=\"#134e4a\"")
            && pbody.contains(r#"style:use-window-font-color="false""#)
            && !pbody.contains("fo:color=\"#000\"")
            && !pbody.contains("fo:color=\"#000000\"")
            && !pbody.contains("fo:color=\"#111827\""),
        "letter body type must stay #134e4a like quote/th, not UA black: {pbody}"
    );
    let default_p = default_style_xml(&styles, "paragraph");
    assert!(
        default_p.contains("fo:color=\"#134e4a\"")
            && default_p.contains(r#"style:use-window-font-color="false""#)
            && !default_p.contains("fo:color=\"#111827\""),
        "Writer default paragraph must stay letter teal, not UA black: {default_p}"
    );
    let standard = office_style_xml(&styles, "Standard");
    assert!(
        standard.contains("fo:color=\"#134e4a\"")
            && standard.contains(r#"style:use-window-font-color="false""#)
            && !standard.contains("fo:color=\"#000000\"")
            && !standard.contains("fo:color=\"#111827\""),
        "Standard must keep letter #134e4a so style-only body is not UA black: {standard}"
    );
    let text_body = office_style_xml(&styles, "Text_20_body");
    assert!(
        text_body.contains("fo:color=\"#134e4a\""),
        "Text body must stay letter #134e4a: {text_body}"
    );
    let plist = style_xml(&content, "Plist", "paragraph");
    let plist_nested = style_xml(&content, "PlistNested", "paragraph");
    let plist_last = style_xml(&content, "PlistLast", "paragraph");
    assert!(
        plist.contains("fo:color=\"#134e4a\"")
            && plist_nested.contains("fo:color=\"#134e4a\"")
            && plist_last.contains("fo:color=\"#134e4a\""),
        "list body type must stay letter teal: {plist} {plist_nested} {plist_last}"
    );
    let tcell = style_xml(&content, "Tcell", "table-cell");
    assert!(
        tcell.contains("fo:color=\"#134e4a\"")
            && tcell.contains(r#"style:use-window-font-color="false""#)
            && !tcell.contains(r#"fo:font-weight="bold""#),
        "table body type must stay letter teal, not th bold or UA black: {tcell}"
    );

    let quote = style_xml(&content, "Quote", "paragraph");
    let quote_border = attr(quote, "fo:border-left").expect("Quote fo:border-left");
    let quote_bar = odf_len_hu(
        quote_border
            .split_whitespace()
            .next()
            .expect("Quote fo:border-left width"),
    );
    assert!(
        (250..450).contains(&quote_bar) && quote_border.ends_with("solid #134e4a"),
        "body color must keep quote 4px #134e4a left bar, got {quote_bar}: {quote}"
    );
    let flow_p = paragraph_containing(&content, "shipment");
    assert!(
        flow_p.contains(r#"<text:span text:style-name="Tsuper">A</text:span>"#)
            && flow_p.contains(r#"<text:span text:style-name="Tsub">2</text:span>"#),
        "body color must keep Tsuper/Tsub: {flow_p}"
    );
    let thcell = style_xml(&content, "THcell", "table-cell");
    assert!(
        thcell.contains("fo:background-color=\"#ccfbf1\"")
            && thcell.contains("solid #0d9488")
            && thcell.contains(r#"fo:font-weight="bold""#),
        "body color must keep THcell teal fill: {thcell}"
    );
    assert!(
        content.contains(r#"<text:list text:style-name="Ltask">"#)
            && content.contains("text:bullet-char=\"\u{2610}\"")
            && content.contains("text:bullet-char=\"\u{2611}\""),
        "body color must keep Writer task boxes: {content}"
    );
    let (width, height) = frame_size_hu(&content);
    assert!(
        width >= MIN_IMAGE_EDGE_HU && height >= MIN_IMAGE_EDGE_HU,
        "draw:frame must keep the 24pt floor, got {width}×{height}: {content}"
    );
    assert!(
        content.contains("<draw:image") && !content.contains("svg:width=\"0.1666in\""),
        "24pt draw:image must not regress to the 16px box: {content}"
    );
    assert!(
        styles.contains(r#"style:display-name="Heading 1""#)
            && styles.contains(r#"style:master-page style:name="Standard""#),
        "body color must keep Writer package styles.xml: {styles}"
    );
}

#[test]
fn letter_md_odt_body_font_like_writer() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("6 September 2026") && text.contains("`prove`"),
        "letter.md must keep body copy and the prove chip Writer paints as Consolas"
    );
    assert!(
        text.contains("```") && text.contains("docagent prove examples/letter.md"),
        "letter.md must keep the prove fence Writer paints as CodeBlock Consolas"
    );
    assert!(
        text.contains("# Northwind Freight") && text.contains("- [x] Receiving dock is clear"),
        "letter.md must keep H1 and tasks"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    let odt = docagent_odt::write(&doc).expect("odt write");
    assert!(docagent_odt::sniff(&odt), "written bytes must sniff as ODT");
    let mut zip = ZipArchive::new(Cursor::new(odt)).expect("odt zip");
    let mut content = String::new();
    zip.by_name("content.xml")
        .expect("content.xml")
        .read_to_string(&mut content)
        .unwrap();
    let mut styles = String::new();
    zip.by_name("styles.xml")
        .expect("styles.xml")
        .read_to_string(&mut styles)
        .unwrap();

    let default_p = default_style_xml(&styles, "paragraph");
    let standard = office_style_xml(&styles, "Standard");
    let heading = office_style_xml(&styles, "Heading");
    let heading1_named = office_style_xml(&styles, "Heading_20_1");
    let text_body = office_style_xml(&styles, "Text_20_body");
    for (name, style) in [
        ("default paragraph", default_p),
        ("Standard", standard),
        ("Heading", heading),
        ("Heading 1", heading1_named),
        ("Text body", text_body),
    ] {
        assert!(
            style.contains(r#"style:font-name="Segoe UI""#)
                && style.contains(r#"fo:font-family="Segoe UI""#),
            "{name} must stay letter Segoe UI, not Writer Liberation Sans: {style}"
        );
        assert!(
            !style.contains(r#"style:font-name="Liberation Sans""#)
                && !style.contains(r#"fo:font-family="Liberation Sans""#)
                && !style.contains(r#"style:font-name="Calibri""#),
            "{name} must not keep Writer Liberation Sans / Word Calibri: {style}"
        );
    }
    assert!(
        styles.contains(r#"style:name="Segoe UI""#)
            && styles.contains(r#"style:name="Consolas""#)
            && !styles.contains(r#"style:font-name="Liberation Sans""#),
        "styles.xml must declare letter Segoe UI + Consolas, not Liberation Sans body: {styles}"
    );

    let pbody = style_xml(&content, "Pbody", "paragraph");
    let heading1 = style_xml(&content, "Heading1", "paragraph");
    let heading2 = style_xml(&content, "Heading2", "paragraph");
    let quote = style_xml(&content, "Quote", "paragraph");
    let plist = style_xml(&content, "Plist", "paragraph");
    let plist_nested = style_xml(&content, "PlistNested", "paragraph");
    let pth = style_xml(&content, "Pth", "paragraph");
    let tlink = style_xml(&content, "Tlink", "text");
    for (name, style) in [
        ("Pbody", pbody),
        ("Heading1", heading1),
        ("Heading2", heading2),
        ("Quote", quote),
        ("Plist", plist),
        ("PlistNested", plist_nested),
        ("Pth", pth),
        ("Tlink", tlink),
    ] {
        assert!(
            style.contains(r#"style:font-name="Segoe UI""#)
                && style.contains(r#"fo:font-family="Segoe UI""#),
            "{name} must stay letter Segoe UI so Writer does not paint Liberation Sans: {style}"
        );
        assert!(
            !style.contains(r#"style:font-name="Liberation Sans""#),
            "{name} must not keep Writer Liberation Sans: {style}"
        );
    }

    let tcode = style_xml(&content, "Tcode", "text");
    let code = style_xml(&content, "CodeBlock", "paragraph");
    for (name, style) in [("Tcode", tcode), ("CodeBlock", code)] {
        assert!(
            style.contains(r#"style:font-name="Consolas""#)
                && style.contains(r#"fo:font-family="Consolas""#),
            "{name} must keep letter Consolas; Writer ignores fo:font-family without style:font-name: {style}"
        );
        assert!(
            !style.contains(r#"style:font-name="Segoe UI""#)
                && !style.contains(r#"style:font-name="Liberation Sans""#),
            "{name} must not inherit body Segoe UI / Liberation Sans: {style}"
        );
    }
    assert!(
        content.contains(r#"<text:span text:style-name="Tcode">prove</text:span>"#)
            && content.contains(r#"text:style-name="CodeBlock""#),
        "letter prove chip/block must stay Tcode/CodeBlock: {content}"
    );

    let heading1_rule = odf_len_hu(
        attr(heading1, "fo:border-bottom")
            .and_then(|v| v.split_whitespace().next())
            .expect("Heading1 fo:border-bottom width"),
    );
    assert!(
        (200..350).contains(&heading1_rule) && heading1.contains("solid #134e4a"),
        "body font must keep Heading1 3px #134e4a rule: {heading1}"
    );
    assert!(
        attr(pbody, "fo:line-height") == Some("115%")
            && attr(heading1, "fo:line-height") == Some("120%"),
        "body font must keep body 115% and heading 120%: {pbody} {heading1}"
    );
    assert!(
        content.contains(r#"<text:list text:style-name="Ltask">"#)
            && content.contains("text:bullet-char=\"\u{2610}\"")
            && content.contains("text:bullet-char=\"\u{2611}\""),
        "body font must keep Writer task boxes: {content}"
    );
    let (width, height) = frame_size_hu(&content);
    assert!(
        width >= MIN_IMAGE_EDGE_HU && height >= MIN_IMAGE_EDGE_HU,
        "draw:frame must keep the 24pt floor, got {width}×{height}: {content}"
    );
}

#[test]
fn letter_md_odt_heading_color_like_writer() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("# Northwind Freight") && text.contains("## Delivery confirmation"),
        "letter.md must keep H1/H2 Writer paints as letter teal, not navy"
    );
    assert!(
        text.contains("6 September 2026") && text.contains("- [x] Receiving dock is clear"),
        "letter.md must keep body copy and tasks"
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

    let odt = docagent_odt::write(&doc).expect("odt write");
    assert!(docagent_odt::sniff(&odt), "written bytes must sniff as ODT");
    let mut zip = ZipArchive::new(Cursor::new(odt)).expect("odt zip");
    let mut content = String::new();
    zip.by_name("content.xml")
        .expect("content.xml")
        .read_to_string(&mut content)
        .unwrap();
    let mut styles = String::new();
    zip.by_name("styles.xml")
        .expect("styles.xml")
        .read_to_string(&mut styles)
        .unwrap();

    let heading1 = style_xml(&content, "Heading1", "paragraph");
    let heading2 = style_xml(&content, "Heading2", "paragraph");
    let heading3 = style_xml(&content, "Heading3", "paragraph");
    assert!(
        heading1.contains("fo:color=\"#134e4a\"")
            && heading2.contains("fo:color=\"#134e4a\"")
            && heading3.contains("fo:color=\"#134e4a\"")
            && heading1.contains(r#"style:use-window-font-color="false""#)
            && heading2.contains(r#"style:use-window-font-color="false""#)
            && heading3.contains(r#"style:use-window-font-color="false""#),
        "letter Heading 1/2/3 type must stay #134e4a like body/quote, not Writer navy: {heading1}"
    );
    assert!(
        !heading1.contains("fo:color=\"#000080\"")
            && !heading1.contains("fo:color=\"#0000ff\"")
            && !heading1.contains("fo:color=\"#2F5496\"")
            && !heading2.contains("fo:color=\"#000080\"")
            && !content.contains("fo:color=\"#000080\""),
        "automatic Heading 1 must not keep Writer navy #000080: {heading1}"
    );
    let heading = office_style_xml(&styles, "Heading");
    let heading1_named = office_style_xml(&styles, "Heading_20_1");
    let heading2_named = office_style_xml(&styles, "Heading_20_2");
    let heading3_named = office_style_xml(&styles, "Heading_20_3");
    assert!(
        heading.contains("fo:color=\"#134e4a\"")
            && heading1_named.contains("fo:color=\"#134e4a\"")
            && heading2_named.contains("fo:color=\"#134e4a\"")
            && heading3_named.contains("fo:color=\"#134e4a\"")
            && heading.contains(r#"style:use-window-font-color="false""#)
            && heading1_named.contains(r#"style:use-window-font-color="false""#)
            && !styles.contains(r#"style:use-window-font-color="true""#),
        "Writer sidebar Heading / Heading 1/2/3 must stay letter teal: {heading}"
    );
    assert!(
        !styles.contains("fo:color=\"#000080\"")
            && !styles.contains("fo:color=\"#0000ff\"")
            && !styles.contains("fo:color=\"#2F5496\""),
        "Writer package Heading must not keep Liberation navy: {styles}"
    );
    assert!(
        content.contains(r#"text:style-name="Heading1""#)
            && content.contains("Northwind Freight"),
        "letter H1 must stay Heading1: {content}"
    );
    let h1_rule = odf_len_hu(
        attr(heading1, "fo:border-bottom")
            .and_then(|v| v.split_whitespace().next())
            .expect("Heading1 fo:border-bottom width"),
    );
    assert!(
        (200..350).contains(&h1_rule) && heading1.contains("solid #134e4a"),
        "heading teal must keep letter 3px #134e4a rule, got {h1_rule}: {heading1}"
    );
    let pbody = style_xml(&content, "Pbody", "paragraph");
    assert!(
        pbody.contains("fo:color=\"#134e4a\"") && attr(pbody, "fo:line-height") == Some("115%"),
        "heading teal must keep body #134e4a and Writer 1.15: {pbody}"
    );
    let quote = style_xml(&content, "Quote", "paragraph");
    assert!(
        attr(quote, "fo:border-left").is_some_and(|v| v.ends_with("solid #134e4a")),
        "heading teal must keep quote 4px #134e4a bar: {quote}"
    );
    let thcell = style_xml(&content, "THcell", "table-cell");
    assert!(
        thcell.contains("fo:background-color=\"#ccfbf1\"")
            && thcell.contains("solid #0d9488")
            && thcell.contains(r#"fo:font-weight="bold""#),
        "heading teal must keep THcell fill: {thcell}"
    );
    assert!(
        content.contains(r#"<text:list text:style-name="Ltask">"#)
            && content.contains("text:bullet-char=\"\u{2610}\"")
            && content.contains("text:bullet-char=\"\u{2611}\""),
        "heading teal must keep Writer task boxes: {content}"
    );
    let (width, height) = frame_size_hu(&content);
    assert!(
        width >= MIN_IMAGE_EDGE_HU && height >= MIN_IMAGE_EDGE_HU,
        "draw:frame must keep the 24pt floor, got {width}×{height}: {content}"
    );
    assert!(
        content.contains("<draw:image") && !content.contains("svg:width=\"0.1666in\""),
        "24pt draw:image must not regress to the 16px box: {content}"
    );
}

#[test]
fn letter_md_odt_heading_tracking_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("# Northwind Freight") && text.contains("## Delivery confirmation"),
        "letter.md must keep H1 Writer paints with letter.css -.02em tracking"
    );
    assert!(
        text.contains("6 September 2026") && text.contains("- [x] Receiving dock is clear"),
        "letter.md must keep body copy and tasks"
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

    let odt = docagent_odt::write(&doc).expect("odt write");
    assert!(docagent_odt::sniff(&odt), "written bytes must sniff as ODT");
    let mut zip = ZipArchive::new(Cursor::new(odt)).expect("odt zip");
    let mut content = String::new();
    zip.by_name("content.xml")
        .expect("content.xml")
        .read_to_string(&mut content)
        .unwrap();
    let mut styles = String::new();
    zip.by_name("styles.xml")
        .expect("styles.xml")
        .read_to_string(&mut styles)
        .unwrap();

    let heading1 = style_xml(&content, "Heading1", "paragraph");
    let heading2 = style_xml(&content, "Heading2", "paragraph");
    assert!(
        heading1.contains(r#"fo:letter-spacing="-0.36pt""#),
        "Heading1 must keep letter.css h1 -.02em tracking (-0.36pt at 18pt): {heading1}"
    );
    assert!(
        attr(heading1, "fo:line-height") == Some("120%")
            && attr(heading2, "fo:line-height") == Some("120%")
            && !heading1.contains(r#"fo:line-height="115%""#)
            && !heading2.contains(r#"fo:line-height="115%""#),
        "h1 tracking must keep letter.css line-height:1.2, not body 115%: {heading1} {heading2}"
    );
    assert!(
        !heading1.contains(r#"fo:letter-spacing="0pt""#)
            && !heading1.contains(r#"fo:letter-spacing="0in""#)
            && !heading1.contains(r#"fo:letter-spacing="normal""#),
        "0pt / normal Heading 1 tracking is WeasyPrint letter-spacing dropped: {heading1}"
    );
    assert!(
        !heading2.contains("fo:letter-spacing="),
        "letter.css letter-spacing is h1-only, not Heading 2: {heading2}"
    );
    let heading1_named = office_style_xml(&styles, "Heading_20_1");
    let heading2_named = office_style_xml(&styles, "Heading_20_2");
    assert!(
        heading1_named.contains(r#"fo:letter-spacing="-0.36pt""#)
            && heading1_named.contains("fo:color=\"#134e4a\"")
            && heading1_named.contains(r#"style:use-window-font-color="false""#),
        "Writer sidebar Heading 1 must keep letter -.02em tracking + teal: {heading1_named}"
    );
    assert!(
        !heading2_named.contains("fo:letter-spacing="),
        "Writer Heading 2 must not pick up h1 tracking: {heading2_named}"
    );
    assert!(
        heading1.contains("fo:color=\"#134e4a\"")
            && heading1.contains(r#"style:use-window-font-color="false""#),
        "h1 tracking must keep heading letter teal: {heading1}"
    );
    let h1_rule = odf_len_hu(
        attr(heading1, "fo:border-bottom")
            .and_then(|v| v.split_whitespace().next())
            .expect("Heading1 fo:border-bottom width"),
    );
    assert!(
        (200..350).contains(&h1_rule) && heading1.contains("solid #134e4a"),
        "h1 tracking must keep letter 3px #134e4a rule, got {h1_rule}: {heading1}"
    );
    let pbody = style_xml(&content, "Pbody", "paragraph");
    assert!(
        pbody.contains("fo:color=\"#134e4a\"") && attr(pbody, "fo:line-height") == Some("115%"),
        "h1 tracking must keep body #134e4a and Writer 1.15: {pbody}"
    );
    let tcell = style_xml(&content, "Tcell", "table-cell");
    let thcell = style_xml(&content, "THcell", "table-cell");
    assert!(
        tcell.contains(r#"fo:font-size="11.4pt""#) && thcell.contains(r#"fo:font-size="11.4pt""#),
        "h1 tracking must keep letter 0.95rem table type: {tcell} {thcell}"
    );
    assert!(
        thcell.contains("fo:background-color=\"#ccfbf1\"")
            && thcell.contains("solid #0d9488")
            && thcell.contains(r#"fo:font-weight="bold""#),
        "h1 tracking must keep THcell fill: {thcell}"
    );
    let quote = style_xml(&content, "Quote", "paragraph");
    assert!(
        attr(quote, "fo:border-left").is_some_and(|v| v.ends_with("solid #134e4a")),
        "h1 tracking must keep quote 4px #134e4a bar: {quote}"
    );
    assert!(
        content.contains(r#"<text:list text:style-name="Ltask">"#)
            && content.contains("text:bullet-char=\"\u{2610}\"")
            && content.contains("text:bullet-char=\"\u{2611}\""),
        "h1 tracking must keep Writer task boxes: {content}"
    );
    let (width, height) = frame_size_hu(&content);
    assert!(
        width >= MIN_IMAGE_EDGE_HU && height >= MIN_IMAGE_EDGE_HU,
        "draw:frame must keep the 24pt floor, got {width}×{height}: {content}"
    );
    assert!(
        content.contains("<draw:image") && !content.contains("svg:width=\"0.1666in\""),
        "24pt draw:image must not regress to the 16px box: {content}"
    );
    let tlink = style_xml(&content, "Tlink", "text");
    assert!(
        tlink.contains("fo:color=\"#0d9488\"")
            && tlink.contains(r#"style:parent-style-name="Internet_20_Link""#),
        "h1 tracking must keep Internet Link teal: {tlink}"
    );
}

#[test]
fn letter_md_odt_heading_leading_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("# Northwind Freight") && text.contains("## Delivery confirmation"),
        "letter.md must keep H1/H2 Writer paints at letter.css line-height:1.2"
    );
    assert!(
        text.contains("6 September 2026") && text.contains("- [x] Receiving dock is clear"),
        "letter.md must keep body copy and tasks"
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

    let odt = docagent_odt::write(&doc).expect("odt write");
    assert!(docagent_odt::sniff(&odt), "written bytes must sniff as ODT");
    let mut zip = ZipArchive::new(Cursor::new(odt)).expect("odt zip");
    let mut content = String::new();
    zip.by_name("content.xml")
        .expect("content.xml")
        .read_to_string(&mut content)
        .unwrap();
    let mut styles = String::new();
    zip.by_name("styles.xml")
        .expect("styles.xml")
        .read_to_string(&mut styles)
        .unwrap();

    let heading1 = style_xml(&content, "Heading1", "paragraph");
    let heading2 = style_xml(&content, "Heading2", "paragraph");
    let heading3 = style_xml(&content, "Heading3", "paragraph");
    assert!(
        attr(heading1, "fo:line-height") == Some("120%"),
        "Heading1 must keep letter.css h1 line-height:1.2 (120%), not body 115%: {heading1}"
    );
    assert!(
        attr(heading2, "fo:line-height") == Some("120%"),
        "Heading2 must keep letter.css h2 line-height:1.2 (120%), not body 115%: {heading2}"
    );
    assert!(
        attr(heading3, "fo:line-height") == Some("120%"),
        "Heading3 must keep letter heading 1.2 leading: {heading3}"
    );
    assert!(
        !heading1.contains(r#"fo:line-height="115%""#)
            && !heading2.contains(r#"fo:line-height="115%""#)
            && !heading3.contains(r#"fo:line-height="115%""#)
            && !heading1.contains(r#"fo:line-height="100%""#)
            && !heading1.contains(r#"fo:line-height="single""#),
        "inherited Writer body 115% / single is jammed heading leading vs WeasyPrint: {heading1}"
    );
    assert!(
        heading1.contains(r#"fo:letter-spacing="-0.36pt""#),
        "h1 leading must keep letter.css -.02em tracking: {heading1}"
    );
    assert!(
        heading1.contains("fo:color=\"#134e4a\"")
            && heading1.contains(r#"style:use-window-font-color="false""#)
            && heading2.contains("fo:color=\"#134e4a\""),
        "h1 leading must keep heading letter teal: {heading1}"
    );
    let heading1_named = office_style_xml(&styles, "Heading_20_1");
    let heading2_named = office_style_xml(&styles, "Heading_20_2");
    let heading3_named = office_style_xml(&styles, "Heading_20_3");
    assert!(
        attr(heading1_named, "fo:line-height") == Some("120%")
            && attr(heading2_named, "fo:line-height") == Some("120%")
            && attr(heading3_named, "fo:line-height") == Some("120%"),
        "Writer sidebar Heading 1/2/3 must keep letter 1.2 leading: {heading1_named}"
    );
    assert!(
        heading1_named.contains(r#"fo:letter-spacing="-0.36pt""#)
            && heading1_named.contains("fo:color=\"#134e4a\""),
        "Writer sidebar Heading 1 must keep letter tracking + teal: {heading1_named}"
    );
    let pbody = style_xml(&content, "Pbody", "paragraph");
    assert!(
        pbody.contains("fo:color=\"#134e4a\"") && attr(pbody, "fo:line-height") == Some("115%"),
        "h1 leading must keep body #134e4a and Writer 1.15: {pbody}"
    );
    assert!(
        !pbody.contains(r#"fo:line-height="120%""#),
        "body 1.15 must not pick up heading 1.2 leading: {pbody}"
    );
    let tcell = style_xml(&content, "Tcell", "table-cell");
    let thcell = style_xml(&content, "THcell", "table-cell");
    assert!(
        tcell.contains(r#"fo:font-size="11.4pt""#) && thcell.contains(r#"fo:font-size="11.4pt""#),
        "h1 leading must keep letter 0.95rem table type: {tcell} {thcell}"
    );
    assert!(
        thcell.contains("fo:background-color=\"#ccfbf1\"")
            && thcell.contains("solid #0d9488")
            && thcell.contains(r#"fo:font-weight="bold""#),
        "h1 leading must keep THcell fill: {thcell}"
    );
    let quote = style_xml(&content, "Quote", "paragraph");
    assert!(
        attr(quote, "fo:border-left").is_some_and(|v| v.ends_with("solid #134e4a")),
        "h1 leading must keep quote 4px #134e4a bar: {quote}"
    );
    assert!(
        content.contains(r#"<text:list text:style-name="Ltask">"#)
            && content.contains("text:bullet-char=\"\u{2610}\"")
            && content.contains("text:bullet-char=\"\u{2611}\""),
        "h1 leading must keep Writer task boxes: {content}"
    );
    let (width, height) = frame_size_hu(&content);
    assert!(
        width >= MIN_IMAGE_EDGE_HU && height >= MIN_IMAGE_EDGE_HU,
        "draw:frame must keep the 24pt floor, got {width}×{height}: {content}"
    );
    assert!(
        content.contains("<draw:image") && !content.contains("svg:width=\"0.1666in\""),
        "24pt draw:image must not regress to the 16px box: {content}"
    );
    let tlink = style_xml(&content, "Tlink", "text");
    assert!(
        tlink.contains("fo:color=\"#0d9488\"")
            && tlink.contains(r#"style:parent-style-name="Internet_20_Link""#),
        "h1 leading must keep Internet Link teal: {tlink}"
    );
}

#[test]
fn letter_md_odt_text_body_line_like_writer() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("6 September 2026"),
        "letter.md must keep the body date Writer paints at Text body 1.15"
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
    let odt = docagent_odt::write(&doc).expect("odt write");
    assert!(docagent_odt::sniff(&odt), "written bytes must sniff as ODT");
    let mut zip = ZipArchive::new(Cursor::new(odt)).expect("odt zip");
    let mut content = String::new();
    zip.by_name("content.xml")
        .expect("content.xml")
        .read_to_string(&mut content)
        .unwrap();
    let mut styles = String::new();
    zip.by_name("styles.xml")
        .expect("styles.xml")
        .read_to_string(&mut styles)
        .unwrap();

    let pbody = style_xml(&content, "Pbody", "paragraph");
    assert!(
        attr(pbody, "fo:line-height") == Some("115%"),
        "Pbody must keep Writer 1.15, not single: {pbody}"
    );
    assert!(
        pbody.contains("fo:color=\"#134e4a\"")
            && !pbody.contains("fo:color=\"#000000\"")
            && !pbody.contains("fo:color=\"#111827\""),
        "body color must stay letter #134e4a: {pbody}"
    );
    let default_p = default_style_xml(&styles, "paragraph");
    let standard = office_style_xml(&styles, "Standard");
    let text_body = office_style_xml(&styles, "Text_20_body");
    assert!(
        attr(default_p, "fo:line-height") == Some("115%"),
        "Writer default paragraph must keep 1.15, not single: {default_p}"
    );
    assert!(
        attr(standard, "fo:line-height") == Some("115%")
            && !standard.contains(r#"fo:line-height="100%""#),
        "Standard must keep Writer 1.15 so style-only body is not jammed single: {standard}"
    );
    assert!(
        attr(text_body, "fo:line-height") == Some("115%"),
        "Text body must keep Writer 1.15: {text_body}"
    );
    assert!(
        standard.contains("fo:color=\"#134e4a\"") && text_body.contains("fo:color=\"#134e4a\""),
        "Writer 1.15 must keep letter #134e4a on Standard/Text body: {standard} {text_body}"
    );
    let plist = style_xml(&content, "Plist", "paragraph");
    let plist_nested = style_xml(&content, "PlistNested", "paragraph");
    assert!(
        attr(plist, "fo:line-height") == Some("115%") && plist.contains("fo:color=\"#134e4a\""),
        "list body must keep Writer 1.15 + letter teal: {plist}"
    );
    assert!(
        attr(plist_nested, "fo:line-height") == Some("115%")
            && plist_nested.contains("fo:color=\"#134e4a\""),
        "nested list body must keep Writer 1.15 + letter teal: {plist_nested}"
    );

    let quote = style_xml(&content, "Quote", "paragraph");
    let quote_border = attr(quote, "fo:border-left").expect("Quote fo:border-left");
    let quote_bar = odf_len_hu(
        quote_border
            .split_whitespace()
            .next()
            .expect("Quote fo:border-left width"),
    );
    assert!(
        (250..450).contains(&quote_bar) && quote_border.ends_with("solid #134e4a"),
        "quote bar must stay letter.html 4px #134e4a, got {quote_bar}: {quote}"
    );
    let thcell = style_xml(&content, "THcell", "table-cell");
    assert!(
        thcell.contains("fo:background-color=\"#ccfbf1\"")
            && thcell.contains("solid #0d9488")
            && thcell.contains(r#"fo:font-weight="bold""#),
        "THcell teal fill must not regress: {thcell}"
    );
    let hop_p = paragraph_containing(&content, "Hop");
    assert!(
        hop_p.contains(r#"text:style-name="Pth""#),
        "Hop header must keep Pth: {hop_p}"
    );
    assert!(
        content.contains(r#"<text:list text:style-name="Ltask">"#)
            && content.contains("text:bullet-char=\"\u{2610}\"")
            && content.contains("text:bullet-char=\"\u{2611}\""),
        "Writer task boxes must not regress: {content}"
    );
    let (width, height) = frame_size_hu(&content);
    assert!(
        width >= MIN_IMAGE_EDGE_HU && height >= MIN_IMAGE_EDGE_HU,
        "draw:frame must keep the 24pt floor, got {width}×{height}: {content}"
    );
    assert!(
        content.contains("<draw:image") && !content.contains("svg:width=\"0.1666in\""),
        "24pt draw:image must not regress to the 16px box: {content}"
    );
    assert!(
        styles.contains(r#"style:display-name="Heading 1""#)
            && styles.contains(r#"style:master-page style:name="Standard""#),
        "Writer 1.15 must keep Writer package styles.xml: {styles}"
    );
}

#[test]
fn letter_md_odt_table_border_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("| Hop | File | Proof |"),
        "letter.md must keep the hop table Writer paints with a 0.5pt #cbd5e1 grid"
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
    let odt = docagent_odt::write(&doc).expect("odt write");
    assert!(docagent_odt::sniff(&odt), "written bytes must sniff as ODT");
    let mut zip = ZipArchive::new(Cursor::new(odt)).expect("odt zip");
    let mut content = String::new();
    zip.by_name("content.xml")
        .expect("content.xml")
        .read_to_string(&mut content)
        .unwrap();
    let mut styles = String::new();
    zip.by_name("styles.xml")
        .expect("styles.xml")
        .read_to_string(&mut styles)
        .unwrap();

    let tcell = style_xml(&content, "Tcell", "table-cell");
    let thcell = style_xml(&content, "THcell", "table-cell");
    assert!(
        tcell.contains(r#"fo:border="0.5pt solid #cbd5e1""#)
            && !tcell.contains(r#"fo:border="0.5pt solid #000000""#),
        "letter hop Tcell must be 0.5pt #cbd5e1, not LibreOffice TableGrid black: {tcell}"
    );
    let th_rule = odf_len_hu(
        attr(thcell, "fo:border-bottom")
            .and_then(|v| v.split_whitespace().next())
            .expect("THcell fo:border-bottom width"),
    );
    assert!(
        thcell.contains(r#"fo:border="0.5pt solid #cbd5e1""#)
            && (120..180).contains(&th_rule)
            && thcell.contains("solid #0d9488")
            && !thcell.contains(r#"fo:border="0.5pt solid #000000""#),
        "THcell must keep 0.5pt #cbd5e1 grid + letter 2px #0d9488 rule, got {th_rule}: {thcell}"
    );
    assert!(
        !content.contains(r#"fo:border="0.5pt solid #000000""#),
        "hop grid must not keep LibreOffice black hairline: {content}"
    );
    let pad_x = cell_pad_hu(tcell, "left");
    let pad_y = cell_pad_hu(tcell, "top");
    assert!(
        pad_x >= 780 && pad_y >= 480 && thcell.contains("fo:background-color=\"#ccfbf1\""),
        "slate grid must keep letter cell pad + THcell fill, got {pad_x}×{pad_y}: {tcell} {thcell}"
    );
    let hop_p = paragraph_containing(&content, "Hop");
    assert!(
        hop_p.contains(r#"text:style-name="Pth""#),
        "Hop header must keep Pth: {hop_p}"
    );
    let pbody = style_xml(&content, "Pbody", "paragraph");
    assert!(
        pbody.contains("fo:color=\"#134e4a\"") && attr(pbody, "fo:line-height") == Some("115%"),
        "slate grid must keep body #134e4a and Writer 1.15: {pbody}"
    );
    let standard = office_style_xml(&styles, "Standard");
    assert!(
        standard.contains("fo:color=\"#134e4a\"") && attr(standard, "fo:line-height") == Some("115%"),
        "slate grid must keep Standard teal + 1.15: {standard}"
    );
    let quote = style_xml(&content, "Quote", "paragraph");
    assert!(
        attr(quote, "fo:border-left")
            .is_some_and(|v| v.ends_with("solid #134e4a")),
        "slate grid must keep quote 4px #134e4a bar: {quote}"
    );
    assert!(
        content.contains(r#"<text:list text:style-name="Ltask">"#)
            && content.contains("text:bullet-char=\"\u{2610}\"")
            && content.contains("text:bullet-char=\"\u{2611}\""),
        "slate grid must keep Writer task boxes: {content}"
    );
    let (width, height) = frame_size_hu(&content);
    assert!(
        width >= MIN_IMAGE_EDGE_HU && height >= MIN_IMAGE_EDGE_HU,
        "draw:frame must keep the 24pt floor, got {width}×{height}: {content}"
    );
    assert!(
        styles.contains(r#"style:display-name="Heading 1""#)
            && styles.contains(r#"style:master-page style:name="Standard""#),
        "slate grid must keep Writer package styles.xml: {styles}"
    );
}

#[test]
fn letter_md_odt_table_type_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("| Hop | File | Proof |"),
        "letter.md must keep the hop table Writer paints at letter 0.95rem"
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
    let odt = docagent_odt::write(&doc).expect("odt write");
    assert!(docagent_odt::sniff(&odt), "written bytes must sniff as ODT");
    let mut zip = ZipArchive::new(Cursor::new(odt)).expect("odt zip");
    let mut content = String::new();
    zip.by_name("content.xml")
        .expect("content.xml")
        .read_to_string(&mut content)
        .unwrap();
    let mut styles = String::new();
    zip.by_name("styles.xml")
        .expect("styles.xml")
        .read_to_string(&mut styles)
        .unwrap();

    let input_p = paragraph_containing(&content, "Input");
    assert!(
        input_p.contains(r#"text:style-name="Pcell""#),
        "letter.md Input must use Pcell so Writer paints 0.95rem, not missing Tcell text-properties: {input_p}"
    );
    let hop_p = paragraph_containing(&content, "Hop");
    assert!(
        hop_p.contains(r#"text:style-name="Pth""#),
        "Hop header must keep Pth 0.95rem: {hop_p}"
    );
    let pcell = style_xml(&content, "Pcell", "paragraph");
    let pth = style_xml(&content, "Pth", "paragraph");
    let tcell = style_xml(&content, "Tcell", "table-cell");
    let thcell = style_xml(&content, "THcell", "table-cell");
    assert!(
        pcell.contains(r#"fo:font-size="11.4pt""#)
            && pth.contains(r#"fo:font-size="11.4pt""#)
            && tcell.contains(r#"fo:font-size="11.4pt""#)
            && thcell.contains(r#"fo:font-size="11.4pt""#),
        "hop table type must be letter CSS 0.95rem (11.4pt), not body 10pt: {pcell} {pth}"
    );
    assert!(
        !pcell.contains(r#"fo:font-size="10pt""#)
            && !pth.contains(r#"fo:font-size="10pt""#)
            && !tcell.contains(r#"fo:font-size="10pt""#)
            && !thcell.contains(r#"fo:font-size="10pt""#)
            && !pcell.contains(r#"fo:font-size="12pt""#)
            && !pcell.contains(r#"fo:font-weight="bold""#),
        "body 10pt / Writer 12pt is the wrong hop-table type: {pcell} {tcell}"
    );
    assert!(
        pcell.contains("fo:color=\"#134e4a\"")
            && pcell.contains(r#"style:use-window-font-color="false""#),
        "Pcell type must stay letter teal: {pcell}"
    );
    assert!(
        thcell.contains("fo:background-color=\"#ccfbf1\"")
            && thcell.contains("solid #0d9488")
            && thcell.contains(r#"fo:font-weight="bold""#),
        "0.95rem table type must keep THcell fill: {thcell}"
    );
    let pbody = style_xml(&content, "Pbody", "paragraph");
    assert!(
        pbody.contains("fo:color=\"#134e4a\"") && attr(pbody, "fo:line-height") == Some("115%"),
        "0.95rem table type must keep body #134e4a and Writer 1.15: {pbody}"
    );
    let quote = style_xml(&content, "Quote", "paragraph");
    assert!(
        attr(quote, "fo:border-left").is_some_and(|v| v.ends_with("solid #134e4a")),
        "0.95rem table type must keep quote 4px #134e4a bar: {quote}"
    );
    assert!(
        content.contains(r#"<text:list text:style-name="Ltask">"#)
            && content.contains("text:bullet-char=\"\u{2610}\"")
            && content.contains("text:bullet-char=\"\u{2611}\""),
        "0.95rem table type must keep Writer task boxes: {content}"
    );
    let (width, height) = frame_size_hu(&content);
    assert!(
        width >= MIN_IMAGE_EDGE_HU && height >= MIN_IMAGE_EDGE_HU,
        "draw:frame must keep the 24pt floor, got {width}×{height}: {content}"
    );
    assert!(
        content.contains("<draw:image") && !content.contains("svg:width=\"0.1666in\""),
        "24pt draw:image must not regress to the 16px box: {content}"
    );
    assert!(
        styles.contains(r#"style:display-name="Heading 1""#)
            && styles.contains(r#"style:master-page style:name="Standard""#),
        "0.95rem table type must keep Writer package styles.xml: {styles}"
    );
}

#[test]
fn letter_md_odt_cell_valign_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("| Hop | File | Proof |") && text.contains("| Input | letter.md |"),
        "letter.md must keep the hop table Writer paints at letter vertical-align top, not UA middle"
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
    let odt = docagent_odt::write(&doc).expect("odt write");
    assert!(docagent_odt::sniff(&odt), "written bytes must sniff as ODT");
    let mut zip = ZipArchive::new(Cursor::new(odt)).expect("odt zip");
    let mut content = String::new();
    zip.by_name("content.xml")
        .expect("content.xml")
        .read_to_string(&mut content)
        .unwrap();
    let mut styles = String::new();
    zip.by_name("styles.xml")
        .expect("styles.xml")
        .read_to_string(&mut styles)
        .unwrap();

    let hop_cell = table_cell_start_containing(&content, "Hop");
    let input_cell = table_cell_start_containing(&content, "Input");
    assert!(
        hop_cell.contains(r#"table:style-name="THcell""#),
        "letter.md Hop header must use THcell: {hop_cell}"
    );
    assert!(
        input_cell.contains(r#"table:style-name="Tcell""#),
        "letter.md Input body must stay Tcell: {input_cell}"
    );
    let tcell = style_xml(&content, "Tcell", "table-cell");
    let thcell = style_xml(&content, "THcell", "table-cell");
    assert!(
        tcell.contains(r#"style:vertical-align="top""#)
            && thcell.contains(r#"style:vertical-align="top""#),
        "letter hop Tcell/THcell must be letter.html th,td vertical-align top: {tcell} {thcell}"
    );
    assert!(
        !tcell.contains(r#"style:vertical-align="middle""#)
            && !thcell.contains(r#"style:vertical-align="middle""#)
            && !tcell.contains(r#"style:vertical-align="automatic""#)
            && !thcell.contains(r#"style:vertical-align="automatic""#)
            && !tcell.contains(r#"style:vertical-align="bottom""#)
            && !thcell.contains(r#"style:vertical-align="bottom""#)
            && !tcell.contains(r#"style:vertical-align="center""#)
            && !thcell.contains(r#"style:vertical-align="center""#),
        "Writer/HTML-import middle hop cells punch sparse bands vs WeasyPrint: {tcell} {thcell}"
    );
    assert!(
        tcell.contains(r#"fo:border="0.5pt solid #cbd5e1""#)
            && thcell.contains(r#"fo:border="0.5pt solid #cbd5e1""#)
            && thcell.contains("fo:background-color=\"#ccfbf1\"")
            && thcell.contains("solid #0d9488")
            && tcell.contains(r#"fo:font-size="11.4pt""#)
            && thcell.contains(r#"fo:font-size="11.4pt""#),
        "cell valign must keep slate grid, THcell fill, and 0.95rem type: {tcell} {thcell}"
    );
    let pbody = style_xml(&content, "Pbody", "paragraph");
    assert!(
        pbody.contains("fo:color=\"#134e4a\"") && attr(pbody, "fo:line-height") == Some("115%"),
        "cell valign must keep body #134e4a and Writer 1.15: {pbody}"
    );
    let quote = style_xml(&content, "Quote", "paragraph");
    assert!(
        attr(quote, "fo:border-left").is_some_and(|v| v.ends_with("solid #134e4a")),
        "cell valign must keep quote 4px #134e4a bar: {quote}"
    );
    assert!(
        content.contains(r#"<text:list text:style-name="Ltask">"#)
            && content.contains("text:bullet-char=\"\u{2610}\"")
            && content.contains("text:bullet-char=\"\u{2611}\""),
        "cell valign must keep Writer task boxes: {content}"
    );
    let (width, height) = frame_size_hu(&content);
    assert!(
        width >= MIN_IMAGE_EDGE_HU && height >= MIN_IMAGE_EDGE_HU,
        "draw:frame must keep the 24pt floor, got {width}×{height}: {content}"
    );
    assert!(
        content.contains("<draw:image") && !content.contains("svg:width=\"0.1666in\""),
        "24pt draw:image must not regress to the 16px box: {content}"
    );
    assert!(
        styles.contains(r#"style:display-name="Heading 1""#)
            && styles.contains(r#"style:master-page style:name="Standard""#),
        "cell valign must keep Writer package styles.xml: {styles}"
    );
}

#[test]
fn letter_md_odt_table_collapse_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("| Hop | File | Proof |") && text.contains("| Input | letter.md |"),
        "letter.md must keep the hop table Writer paints with letter.html border-collapse:collapse"
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
    let odt = docagent_odt::write(&doc).expect("odt write");
    assert!(docagent_odt::sniff(&odt), "written bytes must sniff as ODT");
    let mut zip = ZipArchive::new(Cursor::new(odt)).expect("odt zip");
    let mut content = String::new();
    zip.by_name("content.xml")
        .expect("content.xml")
        .read_to_string(&mut content)
        .unwrap();
    let mut styles = String::new();
    zip.by_name("styles.xml")
        .expect("styles.xml")
        .read_to_string(&mut styles)
        .unwrap();

    let table1 = style_xml(&content, "Table1", "table");
    assert!(
        table1.contains(r#"table:border-model="collapsing""#),
        "letter hop Table1 must be letter.html table{{border-collapse:collapse}}: {table1}"
    );
    assert!(
        !table1.contains(r#"table:border-model="separating""#)
            && !table1.contains(r#"table:border-model="separate""#)
            && !table1.contains("fo:border-spacing=")
            && !table1.contains(r#"fo:border-spacing="0.0277in""#)
            && !table1.contains(r#"fo:border-spacing="2px""#),
        "Writer/HTML-import separating + 2px spacing punches sparse hop gaps vs WeasyPrint: {table1}"
    );
    let table_before = odf_len_hu(
        attr(table1, "fo:margin-top").expect("fo:margin-top on Table1"),
    );
    let table_after = odf_len_hu(
        attr(table1, "fo:margin-bottom").expect("fo:margin-bottom on Table1"),
    );
    assert!(
        (1000..1600).contains(&table_before) && (1000..1600).contains(&table_after),
        "collapsed hop grid must keep letter 1rem (~1200 HU), got {table_before}/{table_after}: {table1}"
    );
    assert!(
        table1.contains(r#"style:rel-width="100%""#),
        "collapsed hop grid must keep letter table width 100%: {table1}"
    );

    let hop_cell = table_cell_start_containing(&content, "Hop");
    let input_cell = table_cell_start_containing(&content, "Input");
    assert!(
        hop_cell.contains(r#"table:style-name="THcell""#),
        "letter.md Hop header must use THcell: {hop_cell}"
    );
    assert!(
        input_cell.contains(r#"table:style-name="Tcell""#),
        "letter.md Input body must stay Tcell: {input_cell}"
    );
    let tcell = style_xml(&content, "Tcell", "table-cell");
    let thcell = style_xml(&content, "THcell", "table-cell");
    assert!(
        tcell.contains(r#"fo:border="0.5pt solid #cbd5e1""#)
            && thcell.contains(r#"fo:border="0.5pt solid #cbd5e1""#)
            && thcell.contains("fo:background-color=\"#ccfbf1\"")
            && thcell.contains("solid #0d9488")
            && tcell.contains(r#"style:vertical-align="top""#)
            && thcell.contains(r#"style:vertical-align="top""#)
            && tcell.contains(r#"fo:font-size="11.4pt""#)
            && thcell.contains(r#"fo:font-size="11.4pt""#),
        "collapsed hop grid must keep slate 0.5pt, THcell fill, valign top, 0.95rem: {tcell} {thcell}"
    );
    let pbody = style_xml(&content, "Pbody", "paragraph");
    assert!(
        pbody.contains("fo:color=\"#134e4a\"") && attr(pbody, "fo:line-height") == Some("115%"),
        "table collapse must keep body #134e4a and Writer 1.15: {pbody}"
    );
    let quote = style_xml(&content, "Quote", "paragraph");
    assert!(
        attr(quote, "fo:border-left").is_some_and(|v| v.ends_with("solid #134e4a")),
        "table collapse must keep quote 4px #134e4a bar: {quote}"
    );
    assert!(
        content.contains(r#"<text:list text:style-name="Ltask">"#)
            && content.contains("text:bullet-char=\"\u{2610}\"")
            && content.contains("text:bullet-char=\"\u{2611}\""),
        "table collapse must keep Writer task boxes: {content}"
    );
    let (width, height) = frame_size_hu(&content);
    assert!(
        width >= MIN_IMAGE_EDGE_HU && height >= MIN_IMAGE_EDGE_HU,
        "draw:frame must keep the 24pt floor, got {width}×{height}: {content}"
    );
    assert!(
        content.contains("<draw:image") && !content.contains("svg:width=\"0.1666in\""),
        "24pt draw:image must not regress to the 16px box: {content}"
    );
    assert!(
        styles.contains(r#"style:display-name="Heading 1""#)
            && styles.contains(r#"style:master-page style:name="Standard""#),
        "table collapse must keep Writer package styles.xml: {styles}"
    );
}

#[test]
fn letter_md_odt_internet_link_like_writer() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
        "letter.md must keep the DocAgent hyperlink Writer paints as Internet Link"
    );
    assert!(
        text.contains("6 September 2026") && text.contains("- [x] Receiving dock is clear"),
        "letter.md must keep body copy and tasks"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    assert!(
        doc.sections[0].body.iter().any(|b| match b {
            Block::Paragraph(p) => p.runs.iter().any(|r| matches!(
                &r.content,
                RunContent::Inline(InlineObject::Hyperlink { display, target })
                    if display == "DocAgent"
                        && target == "https://github.com/kevin9327/docagent"
            )),
            _ => false,
        }),
        "letter.md IR must keep the DocAgent hyperlink"
    );

    let odt = docagent_odt::write(&doc).expect("odt write");
    assert!(docagent_odt::sniff(&odt), "written bytes must sniff as ODT");
    let mut zip = ZipArchive::new(Cursor::new(odt)).expect("odt zip");
    let mut content = String::new();
    zip.by_name("content.xml")
        .expect("content.xml")
        .read_to_string(&mut content)
        .unwrap();
    let mut styles = String::new();
    zip.by_name("styles.xml")
        .expect("styles.xml")
        .read_to_string(&mut styles)
        .unwrap();

    let link_p = paragraph_containing(&content, "DocAgent");
    assert!(
        link_p.contains(r#"<text:a text:style-name="Tlink""#)
            && link_p.contains(r#"xlink:href="https://github.com/kevin9327/docagent""#)
            && link_p.contains("DocAgent</text:a>"),
        "letter link must stay a visible text:a: {link_p}"
    );
    let tlink = style_xml(&content, "Tlink", "text");
    assert!(
        tlink.contains(r#"style:parent-style-name="Internet_20_Link""#)
            && tlink.contains("fo:color=\"#0d9488\"")
            && tlink.contains(r#"style:use-window-font-color="false""#)
            && tlink.contains(r#"style:text-underline-style="solid""#)
            && tlink.contains(r#"style:text-underline-width="auto""#)
            && tlink.contains(r#"style:text-underline-color="font-color""#),
        "Tlink must parent Writer Internet Link and keep letter #0d9488: {tlink}"
    );
    assert!(
        !tlink.contains("fo:color=\"#000080\"")
            && !tlink.contains("fo:color=\"#0000ff\"")
            && !tlink.contains("fo:color=\"#800000\""),
        "automatic Tlink must not keep Writer navy/blue/maroon: {tlink}"
    );

    assert!(
        styles.contains(r#"style:display-name="Internet Link""#)
            && styles.contains(r#"style:name="Internet_20_Link""#)
            && styles.contains(r#"style:display-name="Visited Internet Link""#)
            && styles.contains(r#"style:name="Visited_20_Internet_20_Link""#),
        "styles.xml must include Writer Internet Link + Visited so the sidebar is not navy: {styles}"
    );
    let internet = office_style_xml(&styles, "Internet_20_Link");
    let visited = office_style_xml(&styles, "Visited_20_Internet_20_Link");
    assert!(
        internet.contains("fo:color=\"#0d9488\"")
            && internet.contains(r#"style:use-window-font-color="false""#)
            && internet.contains(r#"style:text-underline-style="solid""#)
            && internet.contains(r#"style:text-underline-width="auto""#)
            && internet.contains(r#"style:text-underline-color="font-color""#),
        "Writer Internet Link must stay letter #0d9488 underline, not navy: {internet}"
    );
    assert!(
        visited.contains("fo:color=\"#0d9488\"")
            && visited.contains(r#"style:use-window-font-color="false""#)
            && visited.contains(r#"style:text-underline-style="solid""#)
            && visited.contains(r#"style:text-underline-width="auto""#)
            && visited.contains(r#"style:text-underline-color="font-color""#),
        "Visited Internet Link must stay letter #0d9488 like a:visited, not maroon: {visited}"
    );
    assert!(
        !internet.contains("fo:color=\"#000080\"")
            && !internet.contains("fo:color=\"#0000ff\"")
            && !visited.contains("fo:color=\"#800000\"")
            && !visited.contains("fo:color=\"#800080\"")
            && !styles.contains("fo:color=\"#000080\"")
            && !styles.contains("fo:color=\"#800000\""),
        "Writer package Internet Link must not keep Liberation navy/maroon: {internet} {visited}"
    );

    let heading1 = style_xml(&content, "Heading1", "paragraph");
    assert!(
        heading1.contains("fo:color=\"#134e4a\"")
            && heading1.contains(r#"style:use-window-font-color="false""#),
        "Internet Link must keep heading letter teal: {heading1}"
    );
    let pbody = style_xml(&content, "Pbody", "paragraph");
    assert!(
        pbody.contains("fo:color=\"#134e4a\"") && attr(pbody, "fo:line-height") == Some("115%"),
        "Internet Link must keep body #134e4a and Writer 1.15: {pbody}"
    );
    let quote = style_xml(&content, "Quote", "paragraph");
    assert!(
        attr(quote, "fo:border-left").is_some_and(|v| v.ends_with("solid #134e4a")),
        "Internet Link must keep quote 4px #134e4a bar: {quote}"
    );
    let thcell = style_xml(&content, "THcell", "table-cell");
    assert!(
        thcell.contains("fo:background-color=\"#ccfbf1\"")
            && thcell.contains("solid #0d9488")
            && thcell.contains(r#"fo:font-weight="bold""#),
        "Internet Link must keep THcell fill: {thcell}"
    );
    assert!(
        content.contains(r#"<text:list text:style-name="Ltask">"#)
            && content.contains("text:bullet-char=\"\u{2610}\"")
            && content.contains("text:bullet-char=\"\u{2611}\""),
        "Internet Link must keep Writer task boxes: {content}"
    );
    let (width, height) = frame_size_hu(&content);
    assert!(
        width >= MIN_IMAGE_EDGE_HU && height >= MIN_IMAGE_EDGE_HU,
        "draw:frame must keep the 24pt floor, got {width}×{height}: {content}"
    );
    assert!(
        content.contains("<draw:image") && !content.contains("svg:width=\"0.1666in\""),
        "24pt draw:image must not regress to the 16px box: {content}"
    );
}

#[test]
fn letter_md_odt_list_indent_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("- [x] Receiving dock is clear")
            && text.contains("Bay 4")
            && text.contains("1. Hash the input"),
        "letter.md must keep tasks, nested Bay 4, and the decimal list Writer paints at 1.25rem"
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

    let odt = docagent_odt::write(&doc).expect("odt write");
    assert!(docagent_odt::sniff(&odt), "written bytes must sniff as ODT");
    let mut zip = ZipArchive::new(Cursor::new(odt)).expect("odt zip");
    let mut content = String::new();
    zip.by_name("content.xml")
        .expect("content.xml")
        .read_to_string(&mut content)
        .unwrap();

    let lbullet = list_style_xml(&content, "Lbullet");
    let lnumber = list_style_xml(&content, "Lnumber");
    let ltask = list_style_xml(&content, "Ltask");
    let ltask_done = list_style_xml(&content, "LtaskDone");
    for (name, style) in [
        ("Lbullet", lbullet),
        ("Lnumber", lnumber),
        ("Ltask", ltask),
        ("LtaskDone", ltask_done),
    ] {
        let left = list_level_left_hu(style, 1);
        let nested = list_level_left_hu(style, 2);
        let deep = list_level_left_hu(style, 3);
        let hang = odf_len_hu(attr(style, "fo:text-indent").expect("fo:text-indent hanging"));
        assert!(
            (1400..1600).contains(&left),
            "{name} level 1 must be letter.css ul,ol padding-left 1.25rem (~1500 HU), not Writer 0.5in, got {left}: {style}"
        );
        assert!(
            (2800..3200).contains(&nested),
            "{name} level 2 must be nested 2×1.25rem (~3000 HU), not Writer 0.75in, got {nested}: {style}"
        );
        assert!(
            (4300..4700).contains(&deep),
            "{name} level 3 must be 3×1.25rem (~4500 HU), not Writer 1in, got {deep}: {style}"
        );
        assert!(
            hang <= -1100 && hang > -1300 && hang.abs() < left,
            "{name} hanging must stay inside the 1.25rem gutter (~-1200 HU), not Writer -0.25in, got {hang}: {style}"
        );
        assert!(
            !style.contains(r#"fo:margin-left="0.5in""#)
                && !style.contains(r#"fo:margin-left="0.75in""#)
                && !style.contains(r#"fo:margin-left="1in""#)
                && !style.contains(r#"fo:text-indent="-0.25in""#),
            "Writer 0.5in/0.75in/1in/-0.25in is a sparse gutter vs WeasyPrint 1.25rem: {style}"
        );
        assert!(
            style.contains(r#"text:list-level-position-and-space-mode="label-alignment""#)
                && !style.contains("text:space-before"),
            "{name} must keep Writer label-alignment: {style}"
        );
    }

    let heading1 = style_xml(&content, "Heading1", "paragraph");
    assert!(
        heading1.contains(r#"fo:letter-spacing="-0.36pt""#)
            && attr(heading1, "fo:line-height") == Some("120%")
            && heading1.contains("fo:color=\"#134e4a\""),
        "list indent must keep h1 tracking, 1.2 leading, and letter teal: {heading1}"
    );
    let pbody = style_xml(&content, "Pbody", "paragraph");
    assert!(
        pbody.contains(r#"style:font-name="Segoe UI""#)
            && pbody.contains("fo:color=\"#134e4a\"")
            && attr(pbody, "fo:line-height") == Some("115%"),
        "list indent must keep Segoe UI body, #134e4a, Writer 1.15: {pbody}"
    );
    let tcode = style_xml(&content, "Tcode", "text");
    assert!(
        tcode.contains(r#"style:font-name="Consolas""#)
            && tcode.contains(r#"fo:font-family="Consolas""#),
        "list indent must keep Consolas code: {tcode}"
    );
    let tcell = style_xml(&content, "Tcell", "table-cell");
    let thcell = style_xml(&content, "THcell", "table-cell");
    assert!(
        tcell.contains(r#"fo:font-size="11.4pt""#) && thcell.contains(r#"fo:font-size="11.4pt""#),
        "list indent must keep letter 0.95rem table type: {tcell} {thcell}"
    );
    assert!(
        thcell.contains("fo:background-color=\"#ccfbf1\"")
            && thcell.contains("solid #0d9488")
            && thcell.contains(r#"fo:font-weight="bold""#),
        "list indent must keep THcell fill: {thcell}"
    );
    let quote = style_xml(&content, "Quote", "paragraph");
    assert!(
        attr(quote, "fo:border-left").is_some_and(|v| v.ends_with("solid #134e4a")),
        "list indent must keep quote 4px #134e4a bar: {quote}"
    );
    let tlink = style_xml(&content, "Tlink", "text");
    assert!(
        tlink.contains("fo:color=\"#0d9488\"")
            && tlink.contains(r#"style:parent-style-name="Internet_20_Link""#),
        "list indent must keep Internet Link teal: {tlink}"
    );
    let tmark = style_xml(&content, "Tmark", "text");
    assert!(
        tmark.contains("fo:background-color=\"#fde68a\"")
            && tmark.contains("fo:color=\"#134e4a\"")
            && tmark.contains(r#"style:use-window-font-color="false""#),
        "list indent must keep Tmark amber + letter teal type: {tmark}"
    );
    assert!(
        content.contains(r#"<text:list text:style-name="Ltask">"#)
            && content.contains("text:bullet-char=\"\u{2610}\"")
            && content.contains("text:bullet-char=\"\u{2611}\""),
        "list indent must keep Writer task boxes: {content}"
    );
    let (width, height) = frame_size_hu(&content);
    assert!(
        width >= MIN_IMAGE_EDGE_HU && height >= MIN_IMAGE_EDGE_HU,
        "draw:frame must keep the 24pt floor, got {width}×{height}: {content}"
    );
    assert!(
        content.contains("<draw:image") && !content.contains("svg:width=\"0.1666in\""),
        "24pt draw:image must not regress to the 16px box: {content}"
    );
}

#[test]
fn letter_md_odt_nested_list_gap_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("- [x] Receiving dock is clear")
            && text.contains("  - Bay 4")
            && text.contains("2. Run Convert")
            && text.contains("   - PDF/^A^"),
        "letter.md must keep nested Bay 4 and nested PDF/HTML Writer paints at 0.25rem"
    );
    assert!(
        text.contains("- [x] Capsule hashes travel with the files"),
        "letter.md must keep consecutive tasks at 0.35rem"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    let odt = docagent_odt::write(&doc).expect("odt write");
    assert!(docagent_odt::sniff(&odt), "written bytes must sniff as ODT");
    let mut zip = ZipArchive::new(Cursor::new(odt)).expect("odt zip");
    let mut content = String::new();
    zip.by_name("content.xml")
        .expect("content.xml")
        .read_to_string(&mut content)
        .unwrap();

    let dock_p = paragraph_containing(&content, "Receiving dock is clear");
    let bay_p = paragraph_containing(&content, "Bay 4");
    let capsule_p = paragraph_containing(&content, "Capsule hashes travel with the files");
    let convert_p = paragraph_containing(&content, "Run Convert");
    assert!(
        dock_p.contains(r#"text:style-name="PlistNested""#),
        "letter.md dock parent of Bay 4 must use PlistNested 0.25rem, not Plist 0.35rem: {dock_p}"
    );
    assert!(
        convert_p.contains(r#"text:style-name="PlistNested""#),
        "letter.md Convert parent of nested PDF/HTML must use PlistNested: {convert_p}"
    );
    assert!(
        capsule_p.contains(r#"text:style-name="Plist""#),
        "consecutive Capsule task must stay Plist 0.35rem: {capsule_p}"
    );
    assert!(
        bay_p.contains(r#"text:style-name="Plist""#),
        "nested Bay 4 item must stay Plist: {bay_p}"
    );

    let plist = style_xml(&content, "Plist", "paragraph");
    let plist_nested = style_xml(&content, "PlistNested", "paragraph");
    let item_after = odf_len_hu(attr(plist, "fo:margin-bottom").expect("Plist fo:margin-bottom"));
    let nested_after = odf_len_hu(
        attr(plist_nested, "fo:margin-bottom").expect("PlistNested fo:margin-bottom"),
    );
    assert!(
        (300..600).contains(&item_after),
        "Plist after must stay letter 0.35rem (~420 HU), got {item_after}: {plist}"
    );
    assert!(
        (200..400).contains(&nested_after) && nested_after < item_after,
        "PlistNested after must be letter.css ul ul 0.25rem (~300 HU), not consecutive 0.35rem {nested_after}: {plist_nested}"
    );
    assert!(
        attr(plist_nested, "fo:line-height") == Some("115%")
            && plist_nested.contains("fo:color=\"#134e4a\""),
        "PlistNested must keep Writer 1.15 + letter teal: {plist_nested}"
    );
    assert!(
        content.contains(r#"<text:list text:style-name="Ltask">"#)
            && content.contains("text:bullet-char=\"\u{2610}\"")
            && content.contains("text:bullet-char=\"\u{2611}\""),
        "nested gap must keep Writer task boxes: {content}"
    );
    let (width, height) = frame_size_hu(&content);
    assert!(
        width >= MIN_IMAGE_EDGE_HU && height >= MIN_IMAGE_EDGE_HU,
        "draw:frame must keep the 24pt floor, got {width}×{height}: {content}"
    );
    assert!(
        content.contains("<draw:image") && !content.contains("svg:width=\"0.1666in\""),
        "24pt draw:image must not regress to the 16px box: {content}"
    );
}

#[test]
fn letter_md_odt_list_after_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("- [ ] Replay the convert instead of hoping")
            && text.contains("3. Compare the capsule")
            && text.contains("1. Hash the input")
            && text.contains("- [x] Capsule hashes travel with the files"),
        "letter.md must keep last-task / last-decimal 1rem vs consecutive li 0.35rem"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    let odt = docagent_odt::write(&doc).expect("odt write");
    assert!(docagent_odt::sniff(&odt), "written bytes must sniff as ODT");
    let mut zip = ZipArchive::new(Cursor::new(odt)).expect("odt zip");
    let mut content = String::new();
    zip.by_name("content.xml")
        .expect("content.xml")
        .read_to_string(&mut content)
        .unwrap();

    let last_task_p = paragraph_containing(&content, "Replay the convert instead of hoping");
    let last_num_p = paragraph_containing(&content, "Compare the capsule");
    let capsule_p = paragraph_containing(&content, "Capsule hashes travel with the files");
    let hash_p = paragraph_containing(&content, "Hash the input");
    let dock_p = paragraph_containing(&content, "Receiving dock is clear");
    let convert_p = paragraph_containing(&content, "Run Convert");
    assert!(
        last_task_p.contains(r#"text:style-name="PlistLast""#),
        "last task must use PlistLast 1rem so ul,ol is not jammed into the next block: {last_task_p}"
    );
    assert!(
        last_num_p.contains(r#"text:style-name="PlistLast""#),
        "last decimal must use PlistLast 1rem: {last_num_p}"
    );
    assert!(
        capsule_p.contains(r#"text:style-name="Plist""#),
        "consecutive Capsule task must stay Plist 0.35rem, not PlistLast 1rem: {capsule_p}"
    );
    assert!(
        hash_p.contains(r#"text:style-name="Plist""#),
        "consecutive Hash decimal must stay Plist 0.35rem, not PlistLast 1rem: {hash_p}"
    );
    assert!(
        dock_p.contains(r#"text:style-name="PlistNested""#)
            && convert_p.contains(r#"text:style-name="PlistNested""#),
        "parents of nested children must keep PlistNested 0.25rem: {dock_p} {convert_p}"
    );
    assert!(
        content.contains(r#"<text:p text:style-name="Plist">HTML</text:p>"#),
        "last nested HTML under Convert must stay Plist 0.35rem, not outer 1rem: {content}"
    );

    let plist = style_xml(&content, "Plist", "paragraph");
    let plist_nested = style_xml(&content, "PlistNested", "paragraph");
    let plist_last = style_xml(&content, "PlistLast", "paragraph");
    let item_after = odf_len_hu(attr(plist, "fo:margin-bottom").expect("Plist fo:margin-bottom"));
    let nested_after = odf_len_hu(
        attr(plist_nested, "fo:margin-bottom").expect("PlistNested fo:margin-bottom"),
    );
    let list_after = odf_len_hu(
        attr(plist_last, "fo:margin-bottom").expect("PlistLast fo:margin-bottom"),
    );
    assert!(
        (300..600).contains(&item_after),
        "Plist after must stay letter 0.35rem (~420 HU), got {item_after}: {plist}"
    );
    assert!(
        (200..400).contains(&nested_after) && nested_after < item_after,
        "PlistNested after must stay letter 0.25rem (~300 HU), got {nested_after}: {plist_nested}"
    );
    assert!(
        (1000..1600).contains(&list_after) && list_after > item_after,
        "PlistLast after must be letter ul,ol 1rem (~1200 HU), larger than consecutive 0.35rem, got {list_after} vs {item_after}: {plist_last}"
    );
    assert!(
        attr(plist_last, "fo:line-height") == Some("115%")
            && plist_last.contains("fo:color=\"#134e4a\""),
        "PlistLast must keep Writer 1.15 + letter teal: {plist_last}"
    );
    assert!(
        content.contains(r#"<text:list text:style-name="Ltask">"#)
            && content.contains("text:bullet-char=\"\u{2610}\"")
            && content.contains("text:bullet-char=\"\u{2611}\""),
        "list after must keep Writer task boxes: {content}"
    );
    let (width, height) = frame_size_hu(&content);
    assert!(
        width >= MIN_IMAGE_EDGE_HU && height >= MIN_IMAGE_EDGE_HU,
        "draw:frame must keep the 24pt floor, got {width}×{height}: {content}"
    );
    assert!(
        content.contains("<draw:image") && !content.contains("svg:width=\"0.1666in\""),
        "24pt draw:image must not regress to the 16px box: {content}"
    );
}

#[test]
fn letter_md_odt_image_before_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("![Harbor mark](docs/assets/mark.png)"),
        "letter.md must keep the Harbor mark so Writer can space 0.6rem before / 0.7rem after"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    let odt = docagent_odt::write(&doc).expect("odt write");
    assert!(docagent_odt::sniff(&odt), "written bytes must sniff as ODT");
    let mut zip = ZipArchive::new(Cursor::new(odt)).expect("odt zip");
    let mut content = String::new();
    zip.by_name("content.xml")
        .expect("content.xml")
        .read_to_string(&mut content)
        .unwrap();

    let mark_p = harbor_paragraph(&content);
    assert!(
        mark_p.contains(r#"text:style-name="Pimage""#),
        "Harbor mark paragraph must use Pimage so the 0.6rem/0.7rem gap is not missing: {mark_p}"
    );
    let pimage = style_xml(&content, "Pimage", "paragraph");
    let before = odf_len_hu(attr(pimage, "fo:margin-top").expect("fo:margin-top on Pimage"));
    let after = odf_len_hu(attr(pimage, "fo:margin-bottom").expect("fo:margin-bottom on Pimage"));
    assert!(
        (500..1000).contains(&before),
        "Pimage before must be letter img 0.6rem (~720 HU), not Writer 0in under H2 {before}: {pimage}"
    );
    assert!(
        (700..1200).contains(&after) && after > before,
        "Pimage after must stay letter p+img 0.7rem (~840 HU), larger than 0.6rem before, got {after} vs {before}: {pimage}"
    );
    assert!(
        !pimage.contains(r#"fo:margin-top="0in""#),
        "Pimage must not keep Writer 0in before the 24pt mark: {pimage}"
    );

    let plist_last = style_xml(&content, "PlistLast", "paragraph");
    let list_after = odf_len_hu(
        attr(plist_last, "fo:margin-bottom").expect("PlistLast fo:margin-bottom"),
    );
    assert!(
        (1000..1600).contains(&list_after),
        "image before must keep PlistLast 1rem (~1200 HU), got {list_after}: {plist_last}"
    );
    let heading1 = style_xml(&content, "Heading1", "paragraph");
    assert!(
        heading1.contains("solid #134e4a"),
        "image before must keep Heading1 3px #134e4a rule: {heading1}"
    );
    assert!(
        content.contains(r#"<text:list text:style-name="Ltask">"#)
            && content.contains("text:bullet-char=\"\u{2610}\"")
            && content.contains("text:bullet-char=\"\u{2611}\""),
        "image before must keep Writer task boxes: {content}"
    );
    let (width, height) = frame_size_hu(&content);
    assert!(
        width >= MIN_IMAGE_EDGE_HU && height >= MIN_IMAGE_EDGE_HU,
        "draw:frame must keep the 24pt floor, got {width}×{height}: {content}"
    );
    assert!(
        content.contains("<draw:image") && !content.contains("svg:width=\"0.1666in\""),
        "24pt draw:image must not regress to the 16px box: {content}"
    );
}

#[test]
fn letter_md_odt_heading2_before_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("# Northwind Freight") && text.contains("## Delivery confirmation"),
        "letter.md must keep H1/H2 so Writer can space letter.css h2 1.1rem before"
    );
    assert!(
        text.contains("![Harbor mark](docs/assets/mark.png)"),
        "letter.md must keep the Harbor mark under H2"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    let odt = docagent_odt::write(&doc).expect("odt write");
    assert!(docagent_odt::sniff(&odt), "written bytes must sniff as ODT");
    let mut zip = ZipArchive::new(Cursor::new(odt)).expect("odt zip");
    let mut content = String::new();
    zip.by_name("content.xml")
        .expect("content.xml")
        .read_to_string(&mut content)
        .unwrap();
    let mut styles = String::new();
    zip.by_name("styles.xml")
        .expect("styles.xml")
        .read_to_string(&mut styles)
        .unwrap();

    let heading2 = style_xml(&content, "Heading2", "paragraph");
    let h2_before = odf_len_hu(
        attr(heading2, "fo:margin-top").expect("fo:margin-top on Heading2"),
    );
    assert!(
        (1100..1600).contains(&h2_before),
        "Heading2 before must be letter.css h2 1.1rem (~1320 HU), not Writer 0.139in {h2_before}: {heading2}"
    );
    assert!(
        !heading2.contains(r#"fo:margin-top="0.139in""#),
        "Heading2 must not keep Writer 0.139in before: {heading2}"
    );
    let h2_after = odf_len_hu(
        attr(heading2, "fo:margin-bottom").expect("fo:margin-bottom on Heading2"),
    );
    assert!(
        (500..580).contains(&h2_after),
        "h2 1.1rem before must keep letter.css h2 0.45rem after (~540 HU), not Writer 0.0835in {h2_after}: {heading2}"
    );
    assert!(
        !heading2.contains(r#"fo:margin-bottom="0.0835in""#),
        "Heading2 must not keep Writer 0.0835in after: {heading2}"
    );
    assert!(
        attr(heading2, "fo:line-height") == Some("120%"),
        "h2 1.1rem before must keep letter.css line-height:1.2: {heading2}"
    );
    let heading2_named = office_style_xml(&styles, "Heading_20_2");
    let named_before = odf_len_hu(
        attr(heading2_named, "fo:margin-top").expect("fo:margin-top on Heading 2"),
    );
    assert!(
        (1100..1600).contains(&named_before),
        "Writer Heading 2 sidebar must keep letter 1.1rem before, got {named_before}: {heading2_named}"
    );
    let named_after = odf_len_hu(
        attr(heading2_named, "fo:margin-bottom").expect("fo:margin-bottom on Heading 2"),
    );
    assert!(
        (500..580).contains(&named_after),
        "Writer Heading 2 sidebar must keep letter 0.45rem after, got {named_after}: {heading2_named}"
    );

    let pimage = style_xml(&content, "Pimage", "paragraph");
    let img_before = odf_len_hu(attr(pimage, "fo:margin-top").expect("fo:margin-top on Pimage"));
    let img_after = odf_len_hu(attr(pimage, "fo:margin-bottom").expect("fo:margin-bottom on Pimage"));
    assert!(
        (500..1000).contains(&img_before) && (700..1200).contains(&img_after) && img_before < img_after,
        "h2 1.1rem must keep Pimage 0.6rem/0.7rem, got {img_before}/{img_after}: {pimage}"
    );
    let plist_last = style_xml(&content, "PlistLast", "paragraph");
    let list_after = odf_len_hu(
        attr(plist_last, "fo:margin-bottom").expect("PlistLast fo:margin-bottom"),
    );
    assert!(
        (1000..1600).contains(&list_after),
        "h2 1.1rem must keep PlistLast 1rem (~1200 HU), got {list_after}: {plist_last}"
    );
    let table1 = style_xml(&content, "Table1", "table");
    assert!(
        table1.contains(r#"table:border-model="collapsing""#),
        "h2 1.1rem must keep collapsing hop grid: {table1}"
    );
    let tcell = style_xml(&content, "Tcell", "table-cell");
    assert!(
        tcell.contains(r#"style:vertical-align="top""#),
        "h2 1.1rem must keep cell valign top: {tcell}"
    );
    let heading1 = style_xml(&content, "Heading1", "paragraph");
    assert!(
        heading1.contains(r#"fo:letter-spacing="-0.36pt""#)
            && attr(heading1, "fo:line-height") == Some("120%"),
        "h2 1.1rem must keep H1 tracking + 120% leading: {heading1}"
    );
    let h1_after = odf_len_hu(
        attr(heading1, "fo:margin-bottom").expect("fo:margin-bottom on Heading1"),
    );
    assert!(
        (680..760).contains(&h1_after) && !heading1.contains(r#"fo:margin-bottom="0.0835in""#),
        "h2 1.1rem must keep letter.css h1 0.6rem after (~720 HU), not Writer 0.0835in {h1_after}: {heading1}"
    );
    assert!(
        content.contains(r#"<text:list text:style-name="Ltask">"#)
            && content.contains("text:bullet-char=\"\u{2610}\"")
            && content.contains("text:bullet-char=\"\u{2611}\""),
        "h2 1.1rem must keep Writer task boxes: {content}"
    );
    let (width, height) = frame_size_hu(&content);
    assert!(
        width >= MIN_IMAGE_EDGE_HU && height >= MIN_IMAGE_EDGE_HU,
        "draw:frame must keep the 24pt floor, got {width}×{height}: {content}"
    );
}

#[test]
fn letter_md_odt_heading2_after_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("# Northwind Freight") && text.contains("## Delivery confirmation"),
        "letter.md must keep H1/H2 so Writer can space letter.css h2 0.45rem after"
    );
    assert!(
        text.contains("![Harbor mark](docs/assets/mark.png)"),
        "letter.md must keep the Harbor mark under H2"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    let odt = docagent_odt::write(&doc).expect("odt write");
    assert!(docagent_odt::sniff(&odt), "written bytes must sniff as ODT");
    let mut zip = ZipArchive::new(Cursor::new(odt)).expect("odt zip");
    let mut content = String::new();
    zip.by_name("content.xml")
        .expect("content.xml")
        .read_to_string(&mut content)
        .unwrap();
    let mut styles = String::new();
    zip.by_name("styles.xml")
        .expect("styles.xml")
        .read_to_string(&mut styles)
        .unwrap();

    let heading2 = style_xml(&content, "Heading2", "paragraph");
    let h2_before = odf_len_hu(
        attr(heading2, "fo:margin-top").expect("fo:margin-top on Heading2"),
    );
    let h2_after = odf_len_hu(
        attr(heading2, "fo:margin-bottom").expect("fo:margin-bottom on Heading2"),
    );
    assert!(
        (500..580).contains(&h2_after),
        "Heading2 after must be letter.css h2 0.45rem (~540 HU), not Writer 0.0835in {h2_after}: {heading2}"
    );
    assert!(
        !heading2.contains(r#"fo:margin-bottom="0.0835in""#),
        "Heading2 must not keep Writer 0.0835in after: {heading2}"
    );
    assert!(
        (1100..1600).contains(&h2_before),
        "h2 0.45rem after must keep letter.css h2 1.1rem (~1320 HU) before, got {h2_before}: {heading2}"
    );
    assert!(
        attr(heading2, "fo:line-height") == Some("120%"),
        "h2 0.45rem after must keep letter.css line-height:1.2: {heading2}"
    );
    let heading2_named = office_style_xml(&styles, "Heading_20_2");
    let named_after = odf_len_hu(
        attr(heading2_named, "fo:margin-bottom").expect("fo:margin-bottom on Heading 2"),
    );
    let named_before = odf_len_hu(
        attr(heading2_named, "fo:margin-top").expect("fo:margin-top on Heading 2"),
    );
    assert!(
        (500..580).contains(&named_after),
        "Writer Heading 2 sidebar must keep letter 0.45rem after, got {named_after}: {heading2_named}"
    );
    assert!(
        (1100..1600).contains(&named_before),
        "Writer Heading 2 sidebar must keep letter 1.1rem before, got {named_before}: {heading2_named}"
    );

    let pimage = style_xml(&content, "Pimage", "paragraph");
    let img_before = odf_len_hu(attr(pimage, "fo:margin-top").expect("fo:margin-top on Pimage"));
    let img_after = odf_len_hu(attr(pimage, "fo:margin-bottom").expect("fo:margin-bottom on Pimage"));
    assert!(
        (500..1000).contains(&img_before) && (700..1200).contains(&img_after) && img_before < img_after,
        "h2 0.45rem after must keep Pimage 0.6rem/0.7rem, got {img_before}/{img_after}: {pimage}"
    );
    let plist_last = style_xml(&content, "PlistLast", "paragraph");
    let list_after = odf_len_hu(
        attr(plist_last, "fo:margin-bottom").expect("PlistLast fo:margin-bottom"),
    );
    assert!(
        (1000..1600).contains(&list_after),
        "h2 0.45rem after must keep PlistLast 1rem (~1200 HU), got {list_after}: {plist_last}"
    );
    let table1 = style_xml(&content, "Table1", "table");
    assert!(
        table1.contains(r#"table:border-model="collapsing""#),
        "h2 0.45rem after must keep collapsing hop grid: {table1}"
    );
    let tcell = style_xml(&content, "Tcell", "table-cell");
    assert!(
        tcell.contains(r#"style:vertical-align="top""#),
        "h2 0.45rem after must keep cell valign top: {tcell}"
    );
    let heading1 = style_xml(&content, "Heading1", "paragraph");
    assert!(
        heading1.contains(r#"fo:letter-spacing="-0.36pt""#)
            && attr(heading1, "fo:line-height") == Some("120%"),
        "h2 0.45rem after must keep H1 tracking + 120% leading: {heading1}"
    );
    let h1_after = odf_len_hu(
        attr(heading1, "fo:margin-bottom").expect("fo:margin-bottom on Heading1"),
    );
    assert!(
        (680..760).contains(&h1_after) && !heading1.contains(r#"fo:margin-bottom="0.0835in""#),
        "h2 0.45rem after must keep letter.css h1 0.6rem after (~720 HU), not Writer 0.0835in {h1_after}: {heading1}"
    );
    assert!(
        content.contains(r#"<text:list text:style-name="Ltask">"#)
            && content.contains("text:bullet-char=\"\u{2610}\"")
            && content.contains("text:bullet-char=\"\u{2611}\""),
        "h2 0.45rem after must keep Writer task boxes: {content}"
    );
    let (width, height) = frame_size_hu(&content);
    assert!(
        width >= MIN_IMAGE_EDGE_HU && height >= MIN_IMAGE_EDGE_HU,
        "draw:frame must keep the 24pt floor, got {width}×{height}: {content}"
    );
}

#[test]
fn letter_md_odt_heading1_before_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("# Northwind Freight") && text.contains("## Delivery confirmation"),
        "letter.md must keep H1/H2 so Writer can space letter.css h1 margin-top 0"
    );
    assert!(
        text.contains("![Harbor mark](docs/assets/mark.png)"),
        "letter.md must keep the Harbor mark under H2"
    );
    assert!(
        text.contains("6 September 2026") && text.contains("operations desk"),
        "letter.md must keep body copy Writer paints with letter.css p 0.7rem after"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    let odt = docagent_odt::write(&doc).expect("odt write");
    assert!(docagent_odt::sniff(&odt), "written bytes must sniff as ODT");
    let mut zip = ZipArchive::new(Cursor::new(odt)).expect("odt zip");
    let mut content = String::new();
    zip.by_name("content.xml")
        .expect("content.xml")
        .read_to_string(&mut content)
        .unwrap();
    let mut styles = String::new();
    zip.by_name("styles.xml")
        .expect("styles.xml")
        .read_to_string(&mut styles)
        .unwrap();

    let heading1 = style_xml(&content, "Heading1", "paragraph");
    let h1_before = odf_len_hu(
        attr(heading1, "fo:margin-top").expect("fo:margin-top on Heading1"),
    );
    let h1_after = odf_len_hu(
        attr(heading1, "fo:margin-bottom").expect("fo:margin-bottom on Heading1"),
    );
    assert!(
        h1_before == 0,
        "Heading1 before must be letter.css h1 margin-top 0, not Writer 0.1665in {h1_before}: {heading1}"
    );
    assert!(
        !heading1.contains(r#"fo:margin-top="0.1665in""#)
            && !heading1.contains(r#"fo:margin-top="0.0277in""#),
        "Writer 0.1665in / IR 200 HU H1 before is sparse vs WeasyPrint margin-top 0: {heading1}"
    );
    assert!(
        (680..760).contains(&h1_after) && !heading1.contains(r#"fo:margin-bottom="0.0835in""#),
        "h1 margin-top 0 must keep letter.css h1 0.6rem after (~720 HU), not Writer 0.0835in {h1_after}: {heading1}"
    );
    assert!(
        heading1.contains(r#"fo:letter-spacing="-0.36pt""#)
            && attr(heading1, "fo:line-height") == Some("120%"),
        "h1 margin-top 0 must keep letter.css tracking + line-height:1.2: {heading1}"
    );
    let heading1_named = office_style_xml(&styles, "Heading_20_1");
    let named_before = odf_len_hu(
        attr(heading1_named, "fo:margin-top").expect("fo:margin-top on Heading 1"),
    );
    let named_after = odf_len_hu(
        attr(heading1_named, "fo:margin-bottom").expect("fo:margin-bottom on Heading 1"),
    );
    assert!(
        named_before == 0 && !heading1_named.contains(r#"fo:margin-top="0.1665in""#),
        "Writer Heading 1 sidebar must keep letter h1 margin-top 0, got {named_before}: {heading1_named}"
    );
    assert!(
        (680..760).contains(&named_after) && !heading1_named.contains(r#"fo:margin-bottom="0.0835in""#),
        "h1 margin-top 0 must keep letter 0.6rem after on Heading 1, got {named_after}: {heading1_named}"
    );

    let heading2 = style_xml(&content, "Heading2", "paragraph");
    let h2_before = odf_len_hu(
        attr(heading2, "fo:margin-top").expect("fo:margin-top on Heading2"),
    );
    let h2_after = odf_len_hu(
        attr(heading2, "fo:margin-bottom").expect("fo:margin-bottom on Heading2"),
    );
    assert!(
        (1100..1600).contains(&h2_before) && (500..580).contains(&h2_after),
        "h1 margin-top 0 must keep letter.css h2 1.1rem/0.45rem, got {h2_before}/{h2_after}: {heading2}"
    );
    assert!(
        attr(heading2, "fo:line-height") == Some("120%"),
        "h1 margin-top 0 must keep letter.css h2 line-height:1.2: {heading2}"
    );

    let pbody = style_xml(&content, "Pbody", "paragraph");
    let body_after = odf_len_hu(
        attr(pbody, "fo:margin-bottom").expect("fo:margin-bottom on Pbody"),
    );
    assert!(
        (800..880).contains(&body_after) && !pbody.contains(r#"fo:margin-bottom="0.0972in""#),
        "h1 margin-top 0 must keep letter.css p 0.7rem after, got {body_after}: {pbody}"
    );
    assert!(
        attr(pbody, "fo:line-height") == Some("115%"),
        "h1 margin-top 0 must keep Writer 1.15: {pbody}"
    );
    let pimage = style_xml(&content, "Pimage", "paragraph");
    let img_before = odf_len_hu(attr(pimage, "fo:margin-top").expect("fo:margin-top on Pimage"));
    let img_after = odf_len_hu(attr(pimage, "fo:margin-bottom").expect("fo:margin-bottom on Pimage"));
    assert!(
        (500..1000).contains(&img_before) && (700..1200).contains(&img_after) && img_before < img_after,
        "h1 margin-top 0 must keep Pimage 0.6rem/0.7rem, got {img_before}/{img_after}: {pimage}"
    );
    let plist_last = style_xml(&content, "PlistLast", "paragraph");
    let list_after = odf_len_hu(
        attr(plist_last, "fo:margin-bottom").expect("PlistLast fo:margin-bottom"),
    );
    assert!(
        (1000..1600).contains(&list_after),
        "h1 margin-top 0 must keep PlistLast 1rem (~1200 HU), got {list_after}: {plist_last}"
    );
    let table1 = style_xml(&content, "Table1", "table");
    assert!(
        table1.contains(r#"table:border-model="collapsing""#),
        "h1 margin-top 0 must keep collapsing hop grid: {table1}"
    );
    let tcell = style_xml(&content, "Tcell", "table-cell");
    assert!(
        tcell.contains(r#"style:vertical-align="top""#),
        "h1 margin-top 0 must keep cell valign top: {tcell}"
    );
    assert!(
        content.contains(r#"<text:list text:style-name="Ltask">"#)
            && content.contains("text:bullet-char=\"\u{2610}\"")
            && content.contains("text:bullet-char=\"\u{2611}\""),
        "h1 margin-top 0 must keep Writer task boxes: {content}"
    );
    let (width, height) = frame_size_hu(&content);
    assert!(
        width >= MIN_IMAGE_EDGE_HU && height >= MIN_IMAGE_EDGE_HU,
        "draw:frame must keep the 24pt floor, got {width}×{height}: {content}"
    );
}

#[test]
fn letter_md_odt_heading1_after_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("# Northwind Freight") && text.contains("## Delivery confirmation"),
        "letter.md must keep H1/H2 so Writer can space letter.css h1 0.6rem after"
    );
    assert!(
        text.contains("![Harbor mark](docs/assets/mark.png)"),
        "letter.md must keep the Harbor mark under H2"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    let odt = docagent_odt::write(&doc).expect("odt write");
    assert!(docagent_odt::sniff(&odt), "written bytes must sniff as ODT");
    let mut zip = ZipArchive::new(Cursor::new(odt)).expect("odt zip");
    let mut content = String::new();
    zip.by_name("content.xml")
        .expect("content.xml")
        .read_to_string(&mut content)
        .unwrap();
    let mut styles = String::new();
    zip.by_name("styles.xml")
        .expect("styles.xml")
        .read_to_string(&mut styles)
        .unwrap();

    let heading1 = style_xml(&content, "Heading1", "paragraph");
    let h1_before = odf_len_hu(
        attr(heading1, "fo:margin-top").expect("fo:margin-top on Heading1"),
    );
    let h1_after = odf_len_hu(
        attr(heading1, "fo:margin-bottom").expect("fo:margin-bottom on Heading1"),
    );
    assert!(
        (680..760).contains(&h1_after),
        "Heading1 after must be letter.css h1 0.6rem (~720 HU), not Writer 0.0835in {h1_after}: {heading1}"
    );
    assert!(
        !heading1.contains(r#"fo:margin-bottom="0.0835in""#),
        "Heading1 must not keep Writer 0.0835in after: {heading1}"
    );
    assert!(
        h1_before == 0 && !heading1.contains(r#"fo:margin-top="0.1665in""#),
        "h1 0.6rem after must keep letter.css h1 margin-top 0, not Writer 0.1665in {h1_before}: {heading1}"
    );
    assert!(
        heading1.contains(r#"fo:letter-spacing="-0.36pt""#)
            && attr(heading1, "fo:line-height") == Some("120%"),
        "h1 0.6rem after must keep letter.css tracking + line-height:1.2: {heading1}"
    );
    let heading1_named = office_style_xml(&styles, "Heading_20_1");
    let named_after = odf_len_hu(
        attr(heading1_named, "fo:margin-bottom").expect("fo:margin-bottom on Heading 1"),
    );
    let named_before = odf_len_hu(
        attr(heading1_named, "fo:margin-top").expect("fo:margin-top on Heading 1"),
    );
    assert!(
        (680..760).contains(&named_after),
        "Writer Heading 1 sidebar must keep letter 0.6rem after, got {named_after}: {heading1_named}"
    );
    assert!(
        named_before == 0 && !heading1_named.contains(r#"fo:margin-top="0.1665in""#),
        "Writer Heading 1 sidebar must keep letter h1 margin-top 0, got {named_before}: {heading1_named}"
    );

    let heading2 = style_xml(&content, "Heading2", "paragraph");
    let h2_before = odf_len_hu(
        attr(heading2, "fo:margin-top").expect("fo:margin-top on Heading2"),
    );
    let h2_after = odf_len_hu(
        attr(heading2, "fo:margin-bottom").expect("fo:margin-bottom on Heading2"),
    );
    assert!(
        (1100..1600).contains(&h2_before) && (500..580).contains(&h2_after),
        "h1 0.6rem after must keep letter.css h2 1.1rem/0.45rem, got {h2_before}/{h2_after}: {heading2}"
    );
    assert!(
        attr(heading2, "fo:line-height") == Some("120%"),
        "h1 0.6rem after must keep letter.css h2 line-height:1.2: {heading2}"
    );

    let pimage = style_xml(&content, "Pimage", "paragraph");
    let img_before = odf_len_hu(attr(pimage, "fo:margin-top").expect("fo:margin-top on Pimage"));
    let img_after = odf_len_hu(attr(pimage, "fo:margin-bottom").expect("fo:margin-bottom on Pimage"));
    assert!(
        (500..1000).contains(&img_before) && (700..1200).contains(&img_after) && img_before < img_after,
        "h1 0.6rem after must keep Pimage 0.6rem/0.7rem, got {img_before}/{img_after}: {pimage}"
    );
    let plist_last = style_xml(&content, "PlistLast", "paragraph");
    let list_after = odf_len_hu(
        attr(plist_last, "fo:margin-bottom").expect("PlistLast fo:margin-bottom"),
    );
    assert!(
        (1000..1600).contains(&list_after),
        "h1 0.6rem after must keep PlistLast 1rem (~1200 HU), got {list_after}: {plist_last}"
    );
    let table1 = style_xml(&content, "Table1", "table");
    assert!(
        table1.contains(r#"table:border-model="collapsing""#),
        "h1 0.6rem after must keep collapsing hop grid: {table1}"
    );
    let tcell = style_xml(&content, "Tcell", "table-cell");
    assert!(
        tcell.contains(r#"style:vertical-align="top""#),
        "h1 0.6rem after must keep cell valign top: {tcell}"
    );
    assert!(
        content.contains(r#"<text:list text:style-name="Ltask">"#)
            && content.contains("text:bullet-char=\"\u{2610}\"")
            && content.contains("text:bullet-char=\"\u{2611}\""),
        "h1 0.6rem after must keep Writer task boxes: {content}"
    );
    let (width, height) = frame_size_hu(&content);
    assert!(
        width >= MIN_IMAGE_EDGE_HU && height >= MIN_IMAGE_EDGE_HU,
        "draw:frame must keep the 24pt floor, got {width}×{height}: {content}"
    );
}

#[test]
fn letter_md_odt_body_after_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("6 September 2026") && text.contains("operations desk"),
        "letter.md must keep body copy Writer paints with letter.css p 0.7rem after"
    );
    assert!(
        text.contains("# Northwind Freight") && text.contains("## Delivery confirmation"),
        "letter.md must keep H1/H2"
    );
    assert!(
        text.contains("![Harbor mark](docs/assets/mark.png)"),
        "letter.md must keep the Harbor mark under H2"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    let odt = docagent_odt::write(&doc).expect("odt write");
    assert!(docagent_odt::sniff(&odt), "written bytes must sniff as ODT");
    let mut zip = ZipArchive::new(Cursor::new(odt)).expect("odt zip");
    let mut content = String::new();
    zip.by_name("content.xml")
        .expect("content.xml")
        .read_to_string(&mut content)
        .unwrap();
    let mut styles = String::new();
    zip.by_name("styles.xml")
        .expect("styles.xml")
        .read_to_string(&mut styles)
        .unwrap();

    assert!(
        content.contains(r#"<text:p text:style-name="Pbody">6 September 2026</text:p>"#),
        "letter body copy must use Pbody so Writer paints 0.7rem, not jammed IR 200: {content}"
    );
    let pbody = style_xml(&content, "Pbody", "paragraph");
    let body_after = odf_len_hu(
        attr(pbody, "fo:margin-bottom").expect("fo:margin-bottom on Pbody"),
    );
    assert!(
        (800..880).contains(&body_after),
        "Pbody after must be letter.css p 0.7rem (~840 HU), not Writer 0.0972in {body_after}: {pbody}"
    );
    assert!(
        !pbody.contains(r#"fo:margin-bottom="0.0972in""#)
            && !pbody.contains(r#"fo:margin-bottom="0.0277in""#)
            && !pbody.contains(r#"fo:margin-bottom="0in""#),
        "Writer 0.0972in / IR 200 HU / 0in Pbody after is the wrong strut vs WeasyPrint 0.7rem: {pbody}"
    );
    assert!(
        attr(pbody, "fo:line-height") == Some("115%"),
        "body 0.7rem after must keep Writer 1.15: {pbody}"
    );
    let text_body = office_style_xml(&styles, "Text_20_body");
    let named_after = odf_len_hu(
        attr(text_body, "fo:margin-bottom").expect("fo:margin-bottom on Text body"),
    );
    assert!(
        (800..880).contains(&named_after) && !text_body.contains(r#"fo:margin-bottom="0.0972in""#),
        "Writer Text body sidebar must keep letter 0.7rem after, got {named_after}: {text_body}"
    );

    let heading2 = style_xml(&content, "Heading2", "paragraph");
    let h2_before = odf_len_hu(
        attr(heading2, "fo:margin-top").expect("fo:margin-top on Heading2"),
    );
    let h2_after = odf_len_hu(
        attr(heading2, "fo:margin-bottom").expect("fo:margin-bottom on Heading2"),
    );
    assert!(
        (1100..1600).contains(&h2_before) && (500..580).contains(&h2_after),
        "body 0.7rem after must keep letter.css h2 1.1rem/0.45rem, got {h2_before}/{h2_after}: {heading2}"
    );
    let heading1 = style_xml(&content, "Heading1", "paragraph");
    let h1_after = odf_len_hu(
        attr(heading1, "fo:margin-bottom").expect("fo:margin-bottom on Heading1"),
    );
    assert!(
        (680..760).contains(&h1_after) && !heading1.contains(r#"fo:margin-bottom="0.0835in""#),
        "body 0.7rem after must keep letter.css h1 0.6rem after (~720 HU), not Writer 0.0835in {h1_after}: {heading1}"
    );
    let h1_before = odf_len_hu(
        attr(heading1, "fo:margin-top").expect("fo:margin-top on Heading1"),
    );
    assert!(
        h1_before == 0 && !heading1.contains(r#"fo:margin-top="0.1665in""#),
        "body 0.7rem after must keep letter.css h1 margin-top 0, not Writer 0.1665in {h1_before}: {heading1}"
    );
    let pimage = style_xml(&content, "Pimage", "paragraph");
    let img_before = odf_len_hu(attr(pimage, "fo:margin-top").expect("fo:margin-top on Pimage"));
    let img_after = odf_len_hu(attr(pimage, "fo:margin-bottom").expect("fo:margin-bottom on Pimage"));
    assert!(
        (500..1000).contains(&img_before) && (700..1200).contains(&img_after) && img_before < img_after,
        "body 0.7rem after must keep Pimage 0.6rem/0.7rem, got {img_before}/{img_after}: {pimage}"
    );
    assert!(
        img_after == body_after,
        "collapsed p+img 0.7rem must match body 0.7rem, got image {img_after} vs Pbody {body_after}: {pimage} {pbody}"
    );
    let plist_last = style_xml(&content, "PlistLast", "paragraph");
    let list_after = odf_len_hu(
        attr(plist_last, "fo:margin-bottom").expect("PlistLast fo:margin-bottom"),
    );
    assert!(
        (1000..1600).contains(&list_after) && list_after > body_after,
        "body 0.7rem after must keep PlistLast 1rem looser than p 0.7rem, got {list_after} vs {body_after}: {plist_last}"
    );
    let table1 = style_xml(&content, "Table1", "table");
    assert!(
        table1.contains(r#"table:border-model="collapsing""#),
        "body 0.7rem after must keep collapsing hop grid: {table1}"
    );
    let tcell = style_xml(&content, "Tcell", "table-cell");
    assert!(
        tcell.contains(r#"style:vertical-align="top""#),
        "body 0.7rem after must keep cell valign top: {tcell}"
    );
    assert!(
        content.contains(r#"<text:list text:style-name="Ltask">"#)
            && content.contains("text:bullet-char=\"\u{2610}\"")
            && content.contains("text:bullet-char=\"\u{2611}\""),
        "body 0.7rem after must keep Writer task boxes: {content}"
    );
    let (width, height) = frame_size_hu(&content);
    assert!(
        width >= MIN_IMAGE_EDGE_HU && height >= MIN_IMAGE_EDGE_HU,
        "draw:frame must keep the 24pt floor, got {width}×{height}: {content}"
    );
}

#[test]
fn letter_md_odt_mark_color_like_writer() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("==three hashes=="),
        "letter.md must keep the ==highlight== Writer paints as Tmark"
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

    let odt = docagent_odt::write(&doc).expect("odt write");
    assert!(docagent_odt::sniff(&odt), "written bytes must sniff as ODT");
    let mut zip = ZipArchive::new(Cursor::new(odt)).expect("odt zip");
    let mut content = String::new();
    zip.by_name("content.xml")
        .expect("content.xml")
        .read_to_string(&mut content)
        .unwrap();
    let mut styles = String::new();
    zip.by_name("styles.xml")
        .expect("styles.xml")
        .read_to_string(&mut styles)
        .unwrap();

    let flow_p = paragraph_containing(&content, "shipment");
    assert!(
        flow_p.contains(r#"<text:span text:style-name="Tmark">three hashes</text:span>"#),
        "letter highlight must be Tmark so Writer shows amber: {flow_p}"
    );
    let tmark = style_xml(&content, "Tmark", "text");
    assert!(
        tmark.contains("fo:background-color=\"#fde68a\""),
        "Tmark must keep letter amber fill: {tmark}"
    );
    assert!(
        tmark.contains("fo:color=\"#134e4a\"")
            && tmark.contains(r#"style:use-window-font-color="false""#)
            && !tmark.contains(r#"style:use-window-font-color="true""#),
        "Tmark type must stay letter.html mark color #134e4a, not UA black on yellow: {tmark}"
    );
    assert!(
        tmark.contains(r#"style:parent-style-name="Mark""#),
        "Tmark must parent Writer Mark so UA Highlight is not black-on-amber: {tmark}"
    );
    let mark_named = office_style_xml(&styles, "Mark");
    assert!(
        mark_named.contains("fo:color=\"#134e4a\"")
            && mark_named.contains("fo:background-color=\"#fde68a\"")
            && mark_named.contains(r#"style:use-window-font-color="false""#),
        "Writer Mark must stay letter teal on amber: {mark_named}"
    );
    assert!(
        !tmark.contains("transparent")
            && !tmark.contains("#ffff00")
            && !tmark.contains("fo:color=\"#000\"")
            && !tmark.contains("fo:color=\"#000000\"")
            && !tmark.contains("fo:color=\"#111827\"")
            && !tmark.contains(r#"fo:padding="0in""#),
        "UA yellow/black mark punches the letter teal chip: {tmark}"
    );
    let tmark_pad_x = odf_len_hu(
        attr(tmark, "fo:padding-left").expect("fo:padding-left on Tmark"),
    );
    let tmark_pad_y = odf_len_hu(
        attr(tmark, "fo:padding")
            .or_else(|| attr(tmark, "fo:padding-top"))
            .expect("fo:padding on Tmark"),
    );
    assert!(
        (150..280).contains(&tmark_pad_x) && (30..90).contains(&tmark_pad_y),
        "Tmark pad must stay letter .05em/.2em, got {tmark_pad_y}×{tmark_pad_x}: {tmark}"
    );

    let tcode = style_xml(&content, "Tcode", "text");
    assert!(
        tcode.contains("fo:background-color=\"#ccfbf1\"")
            && tcode.contains("fo:color=\"#134e4a\"")
            && tcode.contains(r#"style:font-name="Consolas""#),
        "mark color must keep Tcode mint Consolas chip: {tcode}"
    );
    assert!(
        content.contains(r#"<text:list text:style-name="Ltask">"#)
            && content.contains("text:bullet-char=\"\u{2610}\"")
            && content.contains("text:bullet-char=\"\u{2611}\""),
        "mark color must keep Writer task boxes: {content}"
    );
    let (width, height) = frame_size_hu(&content);
    assert!(
        width >= MIN_IMAGE_EDGE_HU && height >= MIN_IMAGE_EDGE_HU,
        "draw:frame must keep the 24pt floor, got {width}×{height}: {content}"
    );
    assert!(
        content.contains("<draw:image") && !content.contains("svg:width=\"0.1666in\""),
        "24pt draw:image must not regress to the 16px box: {content}"
    );
}

#[test]
fn letter_md_odt_page_margin_like_letter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("# Northwind Freight") && text.contains("| Hop | File | Proof |"),
        "letter.md must keep H1 and the hop table Writer paints inside @page 2.5rem/2.75rem"
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
        DEFAULT_MARGIN_HU,
        "letter.md IR must keep Hangul DEFAULT_MARGIN so the codec can remap @page"
    );
    assert_eq!(
        doc.sections[0].page.margin_top,
        DEFAULT_MARGIN_HU,
        "letter.md IR must keep Hangul DEFAULT_MARGIN on top so the codec can remap @page"
    );
    let hangul_measure = A4_WIDTH_HU - 2 * DEFAULT_MARGIN_HU;

    let odt = docagent_odt::write(&doc).expect("odt write");
    assert!(docagent_odt::sniff(&odt), "written bytes must sniff as ODT");
    let mut zip = ZipArchive::new(Cursor::new(odt)).expect("odt zip");
    let mut content = String::new();
    zip.by_name("content.xml")
        .expect("content.xml")
        .read_to_string(&mut content)
        .unwrap();
    let mut styles = String::new();
    zip.by_name("styles.xml")
        .expect("styles.xml")
        .read_to_string(&mut styles)
        .unwrap();

    let page = page_layout_xml(&styles);
    let margin_x = odf_len_hu(attr(page, "fo:margin-left").expect("fo:margin-left on page-layout"));
    let margin_y = odf_len_hu(attr(page, "fo:margin-top").expect("fo:margin-top on page-layout"));
    let margin_right =
        odf_len_hu(attr(page, "fo:margin-right").expect("fo:margin-right on page-layout"));
    let margin_bottom =
        odf_len_hu(attr(page, "fo:margin-bottom").expect("fo:margin-bottom on page-layout"));
    assert_eq!(
        (margin_x, margin_right, margin_y, margin_bottom),
        (3300, 3300, 3000, 3000),
        "letter page-layout must be @page 2.5rem/2.75rem (3000/3300 HU), not Hangul 30mm: {page}"
    );
    assert!(
        margin_x != DEFAULT_MARGIN_HU
            && margin_y != DEFAULT_MARGIN_HU
            && margin_x != 7200
            && margin_x != 5625
            && !page.contains(r#"fo:margin-left="1.1811in""#)
            && !page.contains(r#"fo:margin-left="0.7874in""#)
            && !page.contains(r#"fo:margin-left="1in""#),
        "Hangul 30mm / Writer 2cm / Word 1in / WeasyPrint UA 75px is a sparse Writer frame: {page}"
    );

    let table1 = style_xml(&content, "Table1", "table");
    let hop_width = odf_len_hu(attr(table1, "style:width").expect("style:width on Table1"));
    assert!(
        hop_width > hangul_measure,
        "hop table must span the letter 2.75rem measure, not Hangul 30mm gutters, got {hop_width} vs {hangul_measure}: {table1}"
    );

    let pbody = style_xml(&content, "Pbody", "paragraph");
    assert!(
        pbody.contains("fo:color=\"#134e4a\"") && attr(pbody, "fo:line-height") == Some("115%"),
        "@page must keep body #134e4a and Writer 1.15: {pbody}"
    );
    let heading1 = style_xml(&content, "Heading1", "paragraph");
    assert!(
        heading1.contains(r#"fo:letter-spacing="-0.36pt""#)
            && heading1.contains("fo:color=\"#134e4a\"")
            && heading1.contains("solid #134e4a"),
        "@page must keep Heading1 tracking + teal rule: {heading1}"
    );
    let quote = style_xml(&content, "Quote", "paragraph");
    assert!(
        attr(quote, "fo:border-left").is_some_and(|v| v.ends_with("solid #134e4a")),
        "@page must keep quote 4px #134e4a bar: {quote}"
    );
    let thcell = style_xml(&content, "THcell", "table-cell");
    assert!(
        thcell.contains("fo:background-color=\"#ccfbf1\"")
            && thcell.contains("solid #0d9488")
            && thcell.contains(r#"fo:font-weight="bold""#),
        "@page must keep THcell fill: {thcell}"
    );
    let tmark = style_xml(&content, "Tmark", "text");
    assert!(
        tmark.contains("fo:background-color=\"#fde68a\"")
            && tmark.contains("fo:color=\"#134e4a\"")
            && tmark.contains(r#"style:use-window-font-color="false""#),
        "@page must keep Tmark amber + letter teal type: {tmark}"
    );
    assert!(
        content.contains(r#"<text:list text:style-name="Ltask">"#)
            && content.contains("text:bullet-char=\"\u{2610}\"")
            && content.contains("text:bullet-char=\"\u{2611}\""),
        "@page must keep Writer task boxes: {content}"
    );
    let (width, height) = frame_size_hu(&content);
    assert!(
        width >= MIN_IMAGE_EDGE_HU && height >= MIN_IMAGE_EDGE_HU,
        "draw:frame must keep the 24pt floor, got {width}×{height}: {content}"
    );
    assert!(
        content.contains("<draw:image") && !content.contains("svg:width=\"0.1666in\""),
        "24pt draw:image must not regress to the 16px box: {content}"
    );
    let standard = office_style_xml(&styles, "Standard");
    assert!(
        standard.contains("fo:color=\"#134e4a\"")
            && standard.contains(r#"style:font-name="Segoe UI""#),
        "@page must keep Standard Segoe UI teal: {standard}"
    );
}

fn harbor_paragraph(xml: &str) -> &str {
    let img = xml.find("<draw:image").expect("draw:image in content.xml");
    let start = xml[..img].rfind("<text:p").expect("paragraph wrapping draw:image");
    let end = xml[img..].find("</text:p>").expect("image paragraph close") + img;
    &xml[start..end + 9]
}

fn table_cell_start_containing<'a>(xml: &'a str, needle: &str) -> &'a str {
    let at = xml.find(needle).unwrap_or_else(|| panic!("{needle}"));
    let start = xml[..at]
        .rfind("<table:table-cell")
        .unwrap_or_else(|| panic!("table-cell wrapping {needle}"));
    let end = xml[start..]
        .find('>')
        .map(|i| start + i + 1)
        .unwrap_or_else(|| panic!("table-cell tag close for {needle}"));
    &xml[start..end]
}

fn list_item_start_containing<'a>(xml: &'a str, needle: &str) -> &'a str {
    let at = xml.find(needle).unwrap_or_else(|| panic!("{needle}"));
    let start = xml[..at]
        .rfind("<text:list-item")
        .unwrap_or_else(|| panic!("list-item wrapping {needle}"));
    let end = xml[start..]
        .find('>')
        .map(|i| start + i + 1)
        .unwrap_or_else(|| panic!("list-item tag close for {needle}"));
    &xml[start..end]
}

fn paragraph_containing<'a>(xml: &'a str, needle: &str) -> &'a str {
    let at = xml.find(needle).unwrap_or_else(|| panic!("{needle}"));
    let start = xml[..at]
        .rfind("<text:p")
        .unwrap_or_else(|| panic!("paragraph wrapping {needle}"));
    let end = xml[at..]
        .find("</text:p>")
        .map(|i| at + i + 9)
        .unwrap_or_else(|| panic!("paragraph close for {needle}"));
    &xml[start..end]
}

fn style_xml<'a>(xml: &'a str, name: &str, family: &str) -> &'a str {
    let needle = format!(r#"style:name="{name}" style:family="{family}""#);
    let start = xml.find(&needle).unwrap_or_else(|| panic!("{name} {family} style"));
    let rest = &xml[start..];
    let end = rest.find("</style:style>").expect("style close");
    &rest[..end]
}

fn office_style_xml<'a>(xml: &'a str, name: &str) -> &'a str {
    let needle = format!(r#"style:style style:name="{name}""#);
    let start = xml.find(&needle).unwrap_or_else(|| panic!("{name} style"));
    let rest = &xml[start..];
    let end = rest.find("</style:style>").expect("style close");
    &rest[..end]
}

fn page_layout_xml(xml: &str) -> &str {
    let start = xml
        .find("<style:page-layout-properties")
        .expect("page-layout-properties");
    let rest = &xml[start..];
    let end = rest.find('>').expect("page-layout-properties close");
    &rest[..=end]
}

fn default_style_xml<'a>(xml: &'a str, family: &str) -> &'a str {
    let needle = format!(r#"style:default-style style:family="{family}""#);
    let start = xml
        .find(&needle)
        .unwrap_or_else(|| panic!("default {family} style"));
    let rest = &xml[start..];
    let end = rest
        .find("</style:default-style>")
        .expect("default style close");
    &rest[..end]
}

fn list_style_xml<'a>(xml: &'a str, name: &str) -> &'a str {
    let needle = format!(r#"text:list-style style:name="{name}""#);
    let start = xml
        .find(&needle)
        .unwrap_or_else(|| panic!("{name} list style"));
    let rest = &xml[start..];
    let end = rest.find("</text:list-style>").expect("list style close");
    &rest[..end]
}

fn list_level_left_hu(style: &str, level: u8) -> i32 {
    let needle = format!(r#"text:level="{level}""#);
    let start = style
        .find(&needle)
        .unwrap_or_else(|| panic!("list level {level}: {style}"));
    odf_len_hu(
        attr(&style[start..], "fo:margin-left")
            .unwrap_or_else(|| panic!("fo:margin-left on list level {level}: {style}")),
    )
}

fn cell_pad_hu(style: &str, side: &str) -> i32 {
    let key = format!("fo:padding-{side}");
    let raw = attr(style, &key).or_else(|| attr(style, "fo:padding"));
    odf_len_hu(raw.unwrap_or_else(|| panic!("fo:padding on cell style: {style}")))
}

fn frame_size_hu(xml: &str) -> (i32, i32) {
    let start = xml
        .find("<draw:frame")
        .expect("draw:frame in content.xml");
    let tag = &xml[start..];
    let end = tag.find('>').expect("draw:frame close");
    let tag = &tag[..end];
    let width = odf_len_hu(attr(tag, "svg:width").expect("svg:width"));
    let height = odf_len_hu(attr(tag, "svg:height").expect("svg:height"));
    (width, height)
}

fn attr<'a>(tag: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("{key}=\"");
    let start = tag.find(&needle)? + needle.len();
    let end = tag[start..].find('"')? + start;
    Some(&tag[start..end])
}

fn odf_len_hu(s: &str) -> i32 {
    let s = s.trim();
    if let Some(num) = s.strip_suffix("in") {
        return fixed4_times(num.trim(), 7200);
    }
    if let Some(num) = s.strip_suffix("pt") {
        return fixed4_times(num.trim(), 100);
    }
    panic!("unsupported ODF length {s}");
}

fn fixed4_times(num: &str, unit: i32) -> i32 {
    let neg = num.starts_with('-');
    let num = num.trim_start_matches(['-', '+']);
    let (int, frac) = match num.split_once('.') {
        Some((a, b)) => (a, b),
        None => (num, ""),
    };
    let whole: i32 = if int.is_empty() {
        0
    } else {
        int.parse().expect("odf integer")
    };
    let mut v = i64::from(whole) * 10_000;
    let mut place: i64 = 1_000;
    for ch in frac.chars() {
        if !ch.is_ascii_digit() {
            panic!("odf fraction {num}");
        }
        if place == 0 {
            continue;
        }
        v += i64::from(ch as u8 - b'0') * place;
        place /= 10;
    }
    let mag = i32::try_from((v * i64::from(unit) + 5_000) / 10_000).expect("odf hu");
    if neg { -mag } else { mag }
}
