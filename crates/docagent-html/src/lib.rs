//! Semantic HTML ↔ IR (not from the paint tree). Search/RAG/a11y.

#![forbid(unsafe_code)]

use docagent_model::{
    Block, BreakKind, Color, Document, Float, HU_PER_INCH, HU_PER_POINT, HeaderFooter, ImageData,
    InlineObject, LineSpacing, NumberFormat, NumberingRef, Padding, Paragraph, Run, RunContent,
    Section, Table, TableCell, TableRow, WrapMode,
};

/// Smallest HTML image edge (24pt), matching the PDF on-page floor.
/// 16px at 96dpi is 12pt and paints as a speck.
const MIN_IMAGE_EDGE_PT: i32 = 24;
const MIN_IMAGE_EDGE_HU: i32 = MIN_IMAGE_EDGE_PT * HU_PER_POINT;
const _: () = assert!(MIN_IMAGE_EDGE_PT == 24);
const _: () = assert!(MIN_IMAGE_EDGE_HU == 24 * HU_PER_POINT);

/// Letter CSS `th,td{padding:.4rem .65rem}` at 16px rem / 96dpi: 0.65rem = 780 HU.
const CELL_PAD_X_HU: i32 = 780;
/// 0.4rem = 480 HU.
const CELL_PAD_Y_HU: i32 = 480;
/// Letter CSS `p{margin:0 0 .7rem}` collapsed with `img{margin:.6rem 0}` is 0.7rem.
const IMAGE_AFTER_HU: i32 = 840;
/// Letter CSS `h1{font-size:1.75rem}` at 16px rem / 96dpi: 1.75rem = 2100 HU.
const H1_SIZE_HU: i32 = 2100;
/// Letter CSS `h2{font-size:1.15rem}`: 1.15rem = 1380 HU.
const H2_SIZE_HU: i32 = 1380;
/// WeasyPrint/CSS `line-height:1.45`. Hangul IR 160% is double-spaced on a letter page.
const BODY_LEADING_PCT: u16 = 145;
/// Heading leading, percent of font size (`h1,h2{line-height:1.2}`).
const HEADING_LEADING_PCT: u16 = 120;
/// Letter CSS `ul,ol{padding-left:1.25rem}` at 16px rem / 96dpi: 1.25rem = 1500 HU.
const LIST_INDENT_HU: i32 = 1500;
/// Letter CSS `li{margin:0 0 .35rem}` = 420 HU. IR 80 is jammed.
const LIST_ITEM_AFTER_HU: i32 = 420;
/// Letter CSS `ul,ol{margin:0 0 1rem}` after the last item = 1200 HU.
const LIST_AFTER_HU: i32 = 1200;
/// Letter CSS `blockquote{margin:.5rem 0 1rem}` top = 600 HU. IR 80 is jammed.
const QUOTE_BEFORE_HU: i32 = 600;
/// Letter CSS blockquote margin-bottom 1rem = 1200 HU. IR 200 is jammed.
const QUOTE_AFTER_HU: i32 = 1200;
/// Letter CSS `blockquote{border-left:4px}` at 96dpi = 300 HU.
const QUOTE_BAR_W_HU: i32 = 300;
/// Letter CSS `blockquote{color:#134e4a}` and `border-left:4px solid #134e4a`.
const QUOTE_FG: Color = Color {
    r: 19,
    g: 78,
    b: 74,
    a: 255,
};
/// Letter CSS `pre{margin:.6rem 0 1rem}` top = 720 HU. IR 80 is jammed.
const CODE_BEFORE_HU: i32 = 720;
/// Letter CSS pre margin-bottom 1rem = 1200 HU. IR 200 is jammed.
const CODE_AFTER_HU: i32 = 1200;
/// Letter CSS `code,pre{font-family:ui-monospace,Consolas,monospace}`.
/// `ui-monospace` is a CSS generic; WeasyPrint loads the Consolas face.
const CODE_FONT: &str = "Consolas";
/// Letter CSS `thead th,tr:first-child th{background:#ccfbf1}`.
const TH_BG: Color = Color {
    r: 204,
    g: 251,
    b: 241,
    a: 255,
};
/// Letter CSS `th{color:#134e4a}`.
const TH_FG: Color = Color {
    r: 19,
    g: 78,
    b: 74,
    a: 255,
};
/// Letter CSS `body,article{color:#134e4a}` — not UA black / slate `#111827`.
const BODY_FG: Color = Color {
    r: 19,
    g: 78,
    b: 74,
    a: 255,
};
const _: () = assert!(H1_SIZE_HU == 2100);
const _: () = assert!(H2_SIZE_HU == 1380);
const _: () = assert!(BODY_LEADING_PCT == 145);
const _: () = assert!(HEADING_LEADING_PCT == 120);
const _: () = assert!(LIST_INDENT_HU == 1500);
const _: () = assert!(LIST_ITEM_AFTER_HU == 420);
const _: () = assert!(CELL_PAD_X_HU == 780);
const _: () = assert!(CELL_PAD_Y_HU == 480);
const _: () = assert!(LIST_AFTER_HU == 1200);
const _: () = assert!(QUOTE_BEFORE_HU == 600);
const _: () = assert!(QUOTE_AFTER_HU == 1200);
const _: () = assert!(QUOTE_BAR_W_HU == 300);
const _: () =
    assert!(QUOTE_FG.r == 19 && QUOTE_FG.g == 78 && QUOTE_FG.b == 74 && QUOTE_FG.a == 255);
const _: () = assert!(CODE_BEFORE_HU == 720);
const _: () = assert!(CODE_AFTER_HU == 1200);
const _: () = assert!(CODE_FONT.as_bytes()[0] == b'C');
const _: () = assert!(TH_BG.r == 204 && TH_BG.g == 251 && TH_BG.b == 241 && TH_BG.a == 255);
const _: () = assert!(TH_FG.r == 19 && TH_FG.g == 78 && TH_FG.b == 74 && TH_FG.a == 255);
const _: () = assert!(BODY_FG.r == 19 && BODY_FG.g == 78 && BODY_FG.b == 74 && BODY_FG.a == 255);
/// Letter CSS `article{max-width:48rem}` — ~65ch at 10pt, not a full-bleed column.
const ARTICLE_MAX_WIDTH_REM: u8 = 48;
const _: () = assert!(ARTICLE_MAX_WIDTH_REM == 48);
/// Letter CSS `body{margin:0;background:#f8fafc;color:#134e4a}` — slate around the paper, teal type not UA black.
const BODY_RULE: &str = "body{margin:0;background:#f8fafc;color:#134e4a}";
/// Letter.html WeasyPrint article measure: 48rem column, auto margins, page padding, teal type.
/// Dropping `max-width`/`margin:auto` is a full-bleed sparse page. Dropping `color` is UA black.
const ARTICLE_RULE: &str = r#"article{max-width:48rem;margin:2rem auto;padding:2.5rem 2.75rem;background:#fff;border:1px solid #e2e8f0;font-family:"Segoe UI",Roboto,Helvetica,Arial,sans-serif;font-size:10pt;line-height:1.45;color:#134e4a}"#;
/// Letter CSS `img` 24pt floor. WeasyPrint paints PNG intrinsic 16px (12pt) unless
/// min-* is 24pt **and** the tag names `width`/`height` in pt ([`push_img`]).
const IMG_RULE: &str = "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}";
/// Letter CSS `h1`/`h2` bottom rules — WeasyPrint UA `h1{font-size:2em;margin:.67em 0}`
/// has no teal rule under the title. Pin 3px #134e4a / 2px #0d9488 like letter.html.
const HEADING_RULE: &str = "h1{font-size:1.75rem;line-height:1.2;margin:0 0 .6rem;letter-spacing:-.02em;padding-bottom:.4rem;border-bottom:3px solid #134e4a}h2{font-size:1.15rem;line-height:1.2;margin:1.1rem 0 .45rem;color:#134e4a;padding-bottom:.2rem;border-bottom:2px solid #0d9488}";
/// Letter.html WeasyPrint print margins: same 2.5rem 2.75rem inset as article padding.
/// WeasyPrint UA `@page{margin:75px}` is not letter input.
const PAGE_RULE: &str = "@page{size:A4;margin:2.5rem 2.75rem}";
/// Print drops the screen card chrome; `@page` supplies the letter.html inset.
/// `padding:0` is last so screen `article{padding:2.5rem 2.75rem}` stays the author rule.
const PRINT_RULE: &str =
    "@media print{body{background:#fff}article{margin:0 auto;border:none;padding:0}}";
/// WeasyPrint/print UA `economy` drops th #ccfbf1, code chips, mark amber, pre fill, task boxes.
/// Letter.html keeps those backgrounds (`print-color-adjust:exact`).
const PRINT_COLOR_RULE: &str = "html{-webkit-print-color-adjust:exact;print-color-adjust:exact}";
/// Letter CSS `a{color:#0d9488;text-decoration:underline}`. UA blue / no-underline hides links.
const LINK_RULE: &str = "a{color:#0d9488;text-decoration:underline}";
/// Letter CSS `a:visited{color:#0d9488}` — keep rest teal, not UA purple.
const LINK_VISITED_RULE: &str = "a:visited{color:#0d9488}";
/// Letter CSS `a:hover{color:#134e4a}` — rest teal like body/hr, not UA red/blue hover.
const LINK_HOVER_RULE: &str = "a:hover{color:#134e4a}";
/// Letter CSS `a:focus{color:#134e4a}` — rest teal like hover/body/hr, not UA ring-only.
const LINK_FOCUS_RULE: &str = "a:focus{color:#134e4a}";
/// Letter CSS `table{border-collapse:collapse}` — WeasyPrint UA
/// `table{border-spacing:2px;border-collapse:separate}` paints 2px
/// gaps between hop cells (sparse grid). Pin collapse like letter.html.
const TABLE_RULE: &str = "table{border-collapse:collapse;margin:1rem 0;width:100%}";
/// Letter CSS `thead{display:table-header-group}` — WeasyPrint repeats the hop header.
const THEAD_RULE: &str = "thead{display:table-header-group}";
/// Letter CSS `tbody{display:table-row-group}` — `display:block` table resets stack hop
/// body rows as blocks and punch the 48rem column. Pin row-group like thead.
const TBODY_RULE: &str = "tbody{display:table-row-group}";
/// Letter CSS `th,td{border:1px solid #cbd5e1;padding:.4rem .65rem}` —
/// WeasyPrint UA `td,th{padding:1px}` jams hop cells with no slate grid.
/// Pin `.4rem .65rem` + 1px `#cbd5e1` like letter.html. IR stays
/// [`CELL_PAD_X_HU`]/[`CELL_PAD_Y_HU`].
const CELL_RULE: &str = "th,td{border:1px solid #cbd5e1;padding:.4rem .65rem;text-align:left;font-size:.95rem}";
/// Letter CSS header fill on `thead th` and first-row `th` (tables without `<thead>`).
/// Bare `th{}` paints a body-row th like the hop header.
const TH_RULE: &str = "thead th,tr:first-child th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}";
/// Letter CSS `code{...}` chip. WeasyPrint UA `kbd,samp` is bare monospace / 1px border, not the prove chip.
const KBD_SAMP_RULE: &str = "kbd,samp{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}";
/// Letter CSS `figure` — UA `margin:1em 40px` punches 40px holes in the 48rem column.
/// `figcaption` is body teal, not UA italic/centered sparse caption.
const FIGURE_RULE: &str = "figure{margin:.6rem 0 1rem}figure img{margin:0}figcaption{font-size:.85rem;line-height:1.45;color:#134e4a;margin:.35rem 0 0;text-align:left;font-style:normal}";
/// Letter CSS `caption` — WeasyPrint UA `caption-side:top` + `text-align:center`
/// punches a sparse band above the hop table. Pin bottom/left/teal like figcaption.
const CAPTION_RULE: &str = "caption{caption-side:bottom;text-align:left;font-size:.85rem;line-height:1.45;color:#134e4a;padding:.35rem 0 0;font-style:normal}";
/// Letter CSS `::selection` — UA Highlight (system blue / CanvasText) punches a
/// hole in the teal chip. Pin mint fill + body teal like `th`/`code`/`kbd`.
/// Firefox still needs `::-moz-selection`.
const SELECTION_RULE: &str = "::selection{background:#ccfbf1;color:#134e4a}::-moz-selection{background:#ccfbf1;color:#134e4a}";
/// Letter CSS `abbr` — WeasyPrint UA `abbr[title], acronym[title] { text-decoration: dotted underline }`
/// is uncolored (CanvasText / currentColor can paint black when an inline resets).
/// Pin body-teal dotted so PDF/A · SHA-256 abbreviations stay on the letter chip.
const ABBR_RULE: &str = "abbr[title],acronym[title]{text-decoration:underline dotted #134e4a;text-underline-offset:.15em;cursor:help}";
/// Letter CSS `details`/`summary` — HTML UA `summary{display:list-item}` plus
/// WeasyPrint `disclosure-closed` (▸) paints a marker that punches the 48rem
/// column. Pin block + body teal + no list-style like `ul.tasks`.
const DETAILS_RULE: &str = "details{margin:0 0 .7rem;color:#134e4a}summary{display:block;cursor:pointer;font-weight:700;color:#134e4a;list-style:none}details[open]>summary{list-style:none}";
/// Letter CSS `th,td{vertical-align:top}` — WeasyPrint UA
/// `thead,tbody,tfoot,table>tr{vertical-align:middle}` plus
/// `td,th{vertical-align:inherit}` centers wrapped hop cells in the row
/// and punches sparse bands. Pin top like cell pad `.4rem .65rem`.
const CELL_VALIGN_RULE: &str = "th,td{vertical-align:top}";
/// Letter CSS `ul,ol{margin:0 0 1rem;padding-left:1.25rem}` — WeasyPrint UA
/// `ul,ol{padding-left:40px}` punches a 40px (2.5rem) gutter in the 48rem
/// column. Pin 1.25rem like letter.html agent-replay ol / task ul.
const LIST_RULE: &str = "ul,ol{margin:0 0 1rem;padding-left:1.25rem}";
/// Letter CSS `ul ul,ol ul,ul ol,ol ol{margin:.25rem 0}` — WeasyPrint UA
/// `ul,ol{margin:1em 0}` applies to nested lists too and punches 1em bands
/// between Bay 4 / PDF/A children. Pin 0.25rem like letter.html.
const NESTED_LIST_RULE: &str = "ul ul,ol ul,ul ol,ol ol{margin:.25rem 0}";
/// Letter CSS `li{margin:0 0 .35rem}` — WeasyPrint UA `li{display:list-item}`
/// has no after-margin, so agent-replay / task items jam into each other.
/// Pin 0.35rem like letter.html. IR stays [`LIST_ITEM_AFTER_HU`].
const LI_RULE: &str = "li{margin:0 0 .35rem}";
/// Letter CSS `ul.tasks{list-style:none;padding-left:1.25rem}` — WeasyPrint UA
/// `ul{list-style:disc;padding-left:40px}` paints discs beside GFM checkboxes
/// and a 40px gutter in the 48rem column. Pin no-disc + 1.25rem like
/// [`LIST_RULE`]. Nested Bay 4 stays disc; the checkbox chip is 1.5pt body teal.
const TASKS_RULE: &str = "ul.tasks{list-style:none;padding-left:1.25rem}ul.tasks>li{list-style:none}ul.tasks ul{list-style:disc}ul.tasks input[type=checkbox]{appearance:none;-webkit-appearance:none;display:inline-block;width:.9em;height:.9em;margin-right:.4rem;vertical-align:middle;border:1.5pt solid #134e4a;border-radius:2px;background:#fff;accent-color:#134e4a}ul.tasks input[type=checkbox]:checked{background:#134e4a}";
/// Letter CSS `code{padding:.1em .35em}` chip. WeasyPrint UA `code` is bare
/// monospace with no mint pad, so `prove` paints as sparse body type.
/// `kbd,samp` matches this chip ([`KBD_SAMP_RULE`]); `pre code` zeros the pad.
const CODE_RULE: &str = "code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}";
/// Letter CSS `pre{padding:.75rem 1rem}` mint box. WeasyPrint UA `pre` is
/// `margin:1em 0` with no pad/fill, so the prove fence sits flush and sparse.
/// [`CODE_RULE`] chips `prove`; this pads the fenced command. `pre code` zeros
/// the inherited span pad.
const PRE_RULE: &str = "pre{background:#ccfbf1;padding:.75rem 1rem;margin:.6rem 0 1rem;border-radius:4px;overflow:auto;font-family:ui-monospace,Consolas,monospace}";
/// Letter CSS `pre code{background:transparent;padding:0}` — [`CODE_RULE`]
/// chips `prove` with `.1em .35em` mint pad; without zeroing, that pad
/// insets the prove fence inside [`PRE_RULE`]'s `.75rem 1rem` box and
/// punches a sparse mint band. Pin transparent/zero pad like letter.html.
const PRE_CODE_RULE: &str = "pre code{background:transparent;padding:0;color:#134e4a;font-family:ui-monospace,Consolas,monospace}";
/// Letter CSS `hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}` —
/// WeasyPrint UA `hr{color:gray;border-style:inset;border-width:1px;margin:0.5em auto}`
/// paints a sparse gray hairline. Pin 3px body teal like [`HEADING_RULE`] h1
/// and 1.1rem like letter.html so the hop-table / confirm split stays on the chip.
const HR_RULE: &str = "hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}";
/// Letter CSS `::marker{color:#134e4a}` — WeasyPrint UA
/// `::marker{font-variant-numeric:tabular-nums;white-space:pre;text-transform:none}`
/// has no color, so agent-replay decimals and nested discs paint CanvasText
/// black and punch the teal letter. Pin body teal like `abbr`/`mark`.
const MARKER_RULE: &str = "::marker{color:#134e4a}";
const _: () = assert!(LINK_RULE.as_bytes()[0] == b'a');
const _: () = assert!(LINK_VISITED_RULE.as_bytes()[0] == b'a');
const _: () = assert!(LINK_HOVER_RULE.as_bytes()[0] == b'a');
const _: () = assert!(LINK_FOCUS_RULE.as_bytes()[0] == b'a');
const _: () = assert!(TABLE_RULE.as_bytes()[0] == b't');
const _: () = assert!(THEAD_RULE.as_bytes()[0] == b't');
const _: () = assert!(TBODY_RULE.as_bytes()[0] == b't');
const _: () = assert!(CELL_RULE.as_bytes()[0] == b't');
const _: () = assert!(TH_RULE.as_bytes()[0] == b't');
const _: () = assert!(PRINT_COLOR_RULE.as_bytes()[0] == b'h');
const _: () = assert!(KBD_SAMP_RULE.as_bytes()[0] == b'k');
const _: () = assert!(FIGURE_RULE.as_bytes()[0] == b'f');
const _: () = assert!(CAPTION_RULE.as_bytes()[0] == b'c');
const _: () = assert!(SELECTION_RULE.as_bytes()[0] == b':');
const _: () = assert!(ABBR_RULE.as_bytes()[0] == b'a');
const _: () = assert!(DETAILS_RULE.as_bytes()[0] == b'd');
const _: () = assert!(CELL_VALIGN_RULE.as_bytes()[0] == b't');
const _: () = assert!(HEADING_RULE.as_bytes()[0] == b'h');
const _: () = assert!(LIST_RULE.as_bytes()[0] == b'u');
const _: () = assert!(NESTED_LIST_RULE.as_bytes()[0] == b'u');
const _: () = assert!(LI_RULE.as_bytes()[0] == b'l');
const _: () = assert!(TASKS_RULE.as_bytes()[0] == b'u');
const _: () = assert!(CODE_RULE.as_bytes()[0] == b'c');
const _: () = assert!(PRE_RULE.as_bytes()[0] == b'p');
const _: () = assert!(PRE_CODE_RULE.as_bytes()[0] == b'p');
const _: () = assert!(HR_RULE.as_bytes()[0] == b'h');
const _: () = assert!(MARKER_RULE.as_bytes()[0] == b':');
const _: () = assert!(IMG_RULE.as_bytes()[0] == b'i');
const _: () = assert!(ARTICLE_RULE.as_bytes()[0] == b'a');

fn letter_cell_padding() -> Padding {
    Padding {
        left: CELL_PAD_X_HU,
        right: CELL_PAD_X_HU,
        top: CELL_PAD_Y_HU,
        bottom: CELL_PAD_Y_HU,
    }
}

/// Letter CSS `thead th,tr:first-child th{background:#ccfbf1;color:#134e4a;font-weight:700}`.
fn apply_letter_th_cell(cell: &mut TableCell) {
    cell.shading = Some(TH_BG);
    for b in &mut cell.blocks {
        if let Block::Paragraph(p) = b {
            for run in &mut p.runs {
                run.style.bold = true;
                run.style.color = TH_FG;
            }
        }
    }
}

/// Letter CSS `ul,ol{margin:0 0 1rem}` after the last item; `li{margin:0 0 .35rem}` otherwise.
fn apply_letter_list_spacing(blocks: &mut [Block]) {
    let n = blocks.len();
    for i in 0..n {
        let is_list = matches!(&blocks[i], Block::Paragraph(p) if p.numbering.is_some());
        if !is_list {
            continue;
        }
        let next_list = matches!(
            blocks.get(i + 1),
            Some(Block::Paragraph(q)) if q.numbering.is_some()
        );
        if let Block::Paragraph(p) = &mut blocks[i] {
            p.space_after = if next_list {
                LIST_ITEM_AFTER_HU
            } else {
                LIST_AFTER_HU
            };
        }
    }
}

fn paragraph_is_image_only(p: &Paragraph) -> bool {
    let mut saw_image = false;
    for run in &p.runs {
        match &run.content {
            RunContent::Inline(InlineObject::Image(img)) if img.width > 0 && img.height > 0 => {
                saw_image = true;
            }
            _ => {
                if run.display_text().chars().any(|c| !c.is_whitespace()) {
                    return false;
                }
            }
        }
    }
    saw_image
}

pub fn to_html(doc: &Document) -> String {
    let mut s = String::from(
        r#"<!DOCTYPE html><html lang="en"><head><meta charset="utf-8"/><title>DocAgent</title><style>"#,
    );
    s.push_str(PAGE_RULE);
    s.push_str(PRINT_RULE);
    s.push_str(PRINT_COLOR_RULE);
    s.push_str(BODY_RULE);
    s.push_str(ARTICLE_RULE);
    s.push_str(HEADING_RULE);
    s.push_str("p{margin:0 0 .7rem}");
    s.push_str(IMG_RULE);
    s.push_str(
        r#"blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem;border-left:4px solid #134e4a;color:#134e4a}strong,b{font-weight:700}em,i{font-style:italic}"#,
    );
    s.push_str(LINK_RULE);
    s.push_str(LINK_VISITED_RULE);
    s.push_str(LINK_HOVER_RULE);
    s.push_str(LINK_FOCUS_RULE);
    s.push_str(
        r#"u{text-decoration:underline}sup{font-size:.75em;line-height:normal;vertical-align:super}sub{font-size:.75em;line-height:normal;vertical-align:sub}s,del{text-decoration:line-through;color:#134e4a}mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"#,
    );
    s.push_str(CODE_RULE);
    s.push_str(PRE_RULE);
    s.push_str(PRE_CODE_RULE);
    s.push_str(HR_RULE);
    s.push_str(LIST_RULE);
    s.push_str(NESTED_LIST_RULE);
    s.push_str(LI_RULE);
    s.push_str(TASKS_RULE);
    s.push_str(TABLE_RULE);
    s.push_str(THEAD_RULE);
    s.push_str(TBODY_RULE);
    s.push_str(CELL_RULE);
    s.push_str(TH_RULE);
    s.push_str(KBD_SAMP_RULE);
    s.push_str(FIGURE_RULE);
    s.push_str(CAPTION_RULE);
    s.push_str(SELECTION_RULE);
    s.push_str(ABBR_RULE);
    s.push_str(DETAILS_RULE);
    s.push_str(CELL_VALIGN_RULE);
    s.push_str(MARKER_RULE);
    s.push_str("</style></head><body>");
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

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not html text")]
    NotHtml,
}

pub fn read(bytes: &[u8]) -> Result<Document, Error> {
    let html = std::str::from_utf8(bytes).map_err(|_| Error::NotHtml)?;
    Ok(parse_document(html))
}

struct Cursor<'a> {
    s: &'a str,
    i: usize,
}

enum Token<'a> {
    Text(&'a str),
    Open {
        name: String,
        attrs: Vec<(String, String)>,
        self_closing: bool,
    },
    Close {
        name: String,
    },
}

impl<'a> Cursor<'a> {
    fn new(s: &'a str) -> Self {
        Self { s, i: 0 }
    }

    fn next(&mut self) -> Option<Token<'a>> {
        loop {
            if self.i >= self.s.len() {
                return None;
            }
            if self.s.as_bytes()[self.i] != b'<' {
                let start = self.i;
                while self.i < self.s.len() && self.s.as_bytes()[self.i] != b'<' {
                    self.i += 1;
                }
                return Some(Token::Text(&self.s[start..self.i]));
            }
            if self.skip_special() {
                continue;
            }
            return Some(self.parse_tag());
        }
    }

    fn skip_special(&mut self) -> bool {
        let rest = &self.s[self.i..];
        if rest.len() >= 4 && rest.starts_with("<!--") {
            self.i += 4;
            if let Some(p) = self.s[self.i..].find("-->") {
                self.i += p + 3;
            } else {
                self.i = self.s.len();
            }
            return true;
        }
        if rest.len() >= 2 && rest.as_bytes()[1] == b'!' {
            self.i += 2;
            if let Some(p) = self.s[self.i..].find('>') {
                self.i += p + 1;
            } else {
                self.i = self.s.len();
            }
            return true;
        }
        false
    }

    fn parse_tag(&mut self) -> Token<'a> {
        self.i += 1;
        let close = self.i < self.s.len() && self.s.as_bytes()[self.i] == b'/';
        if close {
            self.i += 1;
        }
        let name_start = self.i;
        while self.i < self.s.len() {
            let b = self.s.as_bytes()[self.i];
            if b.is_ascii_alphanumeric() || b == b'-' {
                self.i += 1;
            } else {
                break;
            }
        }
        let name = self.s[name_start..self.i].to_ascii_lowercase();
        if close {
            self.skip_gt();
            return Token::Close { name };
        }
        let mut attrs = Vec::new();
        let mut self_closing = false;
        loop {
            self.skip_ws();
            if self.i >= self.s.len() {
                break;
            }
            let b = self.s.as_bytes()[self.i];
            if b == b'>' {
                self.i += 1;
                break;
            }
            if b == b'/' {
                self_closing = true;
                self.i += 1;
                continue;
            }
            if let Some(attr) = self.parse_attr() {
                attrs.push(attr);
            } else {
                self.i += 1;
            }
        }
        if is_void(&name) {
            self_closing = true;
        }
        Token::Open {
            name,
            attrs,
            self_closing,
        }
    }

    fn parse_attr(&mut self) -> Option<(String, String)> {
        let start = self.i;
        while self.i < self.s.len() {
            let b = self.s.as_bytes()[self.i];
            if b.is_ascii_alphanumeric() || b == b'-' || b == b':' {
                self.i += 1;
            } else {
                break;
            }
        }
        if start == self.i {
            return None;
        }
        let name = self.s[start..self.i].to_ascii_lowercase();
        self.skip_ws();
        if self.i < self.s.len() && self.s.as_bytes()[self.i] == b'=' {
            self.i += 1;
            self.skip_ws();
            let val = self.parse_attr_value();
            Some((name, unescape(&val)))
        } else {
            Some((name, String::new()))
        }
    }

    fn parse_attr_value(&mut self) -> String {
        if self.i >= self.s.len() {
            return String::new();
        }
        let q = self.s.as_bytes()[self.i];
        if q == b'"' || q == b'\'' {
            self.i += 1;
            let start = self.i;
            while self.i < self.s.len() && self.s.as_bytes()[self.i] != q {
                self.i += 1;
            }
            let v = self.s[start..self.i].to_string();
            if self.i < self.s.len() {
                self.i += 1;
            }
            v
        } else {
            let start = self.i;
            while self.i < self.s.len() {
                let b = self.s.as_bytes()[self.i];
                if b.is_ascii_whitespace() || b == b'>' || b == b'/' {
                    break;
                }
                self.i += 1;
            }
            self.s[start..self.i].to_string()
        }
    }

    fn skip_ws(&mut self) {
        while self.i < self.s.len() && self.s.as_bytes()[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn skip_gt(&mut self) {
        if let Some(p) = self.s[self.i..].find('>') {
            self.i += p + 1;
        } else {
            self.i = self.s.len();
        }
    }

    fn skip_to_close_raw(&mut self, name: &str) {
        let rest = &self.s[self.i..];
        let bytes = rest.as_bytes();
        let tag = name.as_bytes();
        let mut i = 0;
        while i + 2 + tag.len() <= bytes.len() {
            if bytes[i] == b'<'
                && bytes[i + 1] == b'/'
                && bytes[i + 2..i + 2 + tag.len()].eq_ignore_ascii_case(tag)
            {
                let after = i + 2 + tag.len();
                if after == bytes.len()
                    || bytes[after] == b'>'
                    || bytes[after].is_ascii_whitespace()
                {
                    self.i += after;
                    self.skip_gt();
                    return;
                }
            }
            i += 1;
        }
        self.i = self.s.len();
    }
}

fn is_void(name: &str) -> bool {
    matches!(
        name,
        "img"
            | "br"
            | "hr"
            | "input"
            | "meta"
            | "link"
            | "area"
            | "base"
            | "col"
            | "embed"
            | "source"
            | "track"
            | "wbr"
    )
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';') else {
            out.push_str(rest);
            return out;
        };
        let ent = &rest[..=end];
        match ent {
            "&amp;" => out.push('&'),
            "&lt;" => out.push('<'),
            "&gt;" => out.push('>'),
            "&quot;" | "&#34;" => out.push('"'),
            "&apos;" | "&#39;" => out.push('\''),
            _ => out.push_str(ent),
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

fn parse_document(html: &str) -> Document {
    let mut cur = Cursor::new(html);
    let mut doc = Document::new();
    let mut loose = Section::default();
    while let Some(tok) = cur.next() {
        match tok {
            Token::Open {
                name,
                attrs,
                self_closing,
            } => match name.as_str() {
                "head" | "style" | "script" => {
                    if !self_closing {
                        cur.skip_to_close_raw(&name);
                    }
                }
                "article" => doc.sections.push(parse_article(&mut cur)),
                "html" | "body" => {}
                "header" => {
                    loose.header = Some(header_footer(parse_blocks(&mut cur, "header")));
                }
                "footer" => {
                    loose.footer = Some(header_footer(parse_blocks(&mut cur, "footer")));
                }
                _ => {
                    if let Some(more) = open_to_blocks(&mut cur, &name, &attrs, self_closing) {
                        loose.body.extend(more);
                    }
                }
            },
            Token::Text(t) => {
                if let Some(p) = text_para(t) {
                    loose.body.push(Block::Paragraph(p));
                }
            }
            Token::Close { .. } => {}
        }
    }
    if doc.sections.is_empty() {
        doc.sections.push(loose);
    } else if !loose.body.is_empty() || loose.header.is_some() || loose.footer.is_some() {
        doc.sections.insert(0, loose);
    }
    for section in &mut doc.sections {
        apply_letter_list_spacing(&mut section.body);
        if let Some(h) = &mut section.header {
            apply_letter_list_spacing(&mut h.blocks);
        }
        if let Some(f) = &mut section.footer {
            apply_letter_list_spacing(&mut f.blocks);
        }
    }
    doc
}

fn header_footer(blocks: Vec<Block>) -> HeaderFooter {
    HeaderFooter {
        blocks,
        different_first: None,
        different_odd_even: None,
    }
}

fn parse_article(cur: &mut Cursor<'_>) -> Section {
    let mut section = Section::default();
    loop {
        match cur.next() {
            None => break,
            Some(Token::Close { name }) if name == "article" => break,
            Some(Token::Open {
                name,
                attrs,
                self_closing,
            }) => match name.as_str() {
                "head" | "style" | "script" => {
                    if !self_closing {
                        cur.skip_to_close_raw(&name);
                    }
                }
                "header" => {
                    section.header = Some(header_footer(parse_blocks(cur, "header")));
                }
                "footer" => {
                    section.footer = Some(header_footer(parse_blocks(cur, "footer")));
                }
                _ => {
                    if let Some(more) = open_to_blocks(cur, &name, &attrs, self_closing) {
                        section.body.extend(more);
                    }
                }
            },
            Some(Token::Text(t)) => {
                if let Some(p) = text_para(t) {
                    section.body.push(Block::Paragraph(p));
                }
            }
            Some(Token::Close { .. }) => {}
        }
    }
    section
}

fn parse_blocks(cur: &mut Cursor<'_>, until: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    loop {
        match cur.next() {
            None => break,
            Some(Token::Close { name }) if name == until => break,
            Some(Token::Open {
                name,
                attrs,
                self_closing,
            }) => match name.as_str() {
                "head" | "style" | "script" => {
                    if !self_closing {
                        cur.skip_to_close_raw(&name);
                    }
                }
                _ => {
                    if let Some(more) = open_to_blocks(cur, &name, &attrs, self_closing) {
                        blocks.extend(more);
                    }
                }
            },
            Some(Token::Text(t)) => {
                if let Some(p) = text_para(t) {
                    blocks.push(Block::Paragraph(p));
                }
            }
            Some(Token::Close { .. }) => {}
        }
    }
    blocks
}

fn open_to_blocks(
    cur: &mut Cursor<'_>,
    name: &str,
    attrs: &[(String, String)],
    self_closing: bool,
) -> Option<Vec<Block>> {
    match name {
        "p" => Some(vec![Block::Paragraph(para_until(
            cur,
            "p",
            None,
            false,
            false,
            self_closing,
        ))]),
        "h1" => Some(vec![Block::Paragraph(para_until(
            cur,
            "h1",
            Some(1),
            false,
            false,
            self_closing,
        ))]),
        "h2" => Some(vec![Block::Paragraph(para_until(
            cur,
            "h2",
            Some(2),
            false,
            false,
            self_closing,
        ))]),
        "h3" => Some(vec![Block::Paragraph(para_until(
            cur,
            "h3",
            Some(3),
            false,
            false,
            self_closing,
        ))]),
        "blockquote" => Some(vec![Block::Paragraph(para_until(
            cur,
            "blockquote",
            None,
            true,
            false,
            self_closing,
        ))]),
        "pre" => Some(vec![Block::Paragraph(para_until(
            cur,
            "pre",
            None,
            false,
            true,
            self_closing,
        ))]),
        "hr" => Some(vec![Block::Break(BreakKind::Thematic)]),
        "br" => None,
        "img" => image_from_attrs(attrs).map(|img| vec![Block::Paragraph(image_para(img))]),
        "aside" => Some(aside_blocks(parse_blocks(cur, "aside"))),
        "ul" => Some(parse_list(cur, list_fmt(attrs), 1, 0)),
        "ol" => {
            let start = attr(attrs, "start")
                .and_then(|s| s.parse().ok())
                .unwrap_or(1);
            Some(parse_list(cur, NumberFormat::Decimal, start, 0))
        }
        "table" => Some(vec![Block::Table(parse_table(cur))]),
        "section" | "div" | "main" | "nav" => Some(parse_blocks(cur, name)),
        _ if self_closing => None,
        _ => Some(parse_blocks(cur, name)),
    }
}

fn para_until(
    cur: &mut Cursor<'_>,
    until: &str,
    outline: Option<u8>,
    quote: bool,
    code: bool,
    self_closing: bool,
) -> Paragraph {
    let runs = if self_closing {
        Vec::new()
    } else {
        parse_inlines(cur, until).runs
    };
    paragraph_from_runs(runs, outline, quote, code)
}

fn paragraph_from_runs(runs: Vec<Run>, outline: Option<u8>, quote: bool, code: bool) -> Paragraph {
    let mut p = Paragraph::from_text("");
    p.runs = if runs.is_empty() {
        vec![Run::text("")]
    } else {
        runs
    };
    p.outline_level = outline;
    p.quote = quote;
    p.code_block = code;
    apply_letter_type(&mut p);
    apply_letter_body_color(&mut p);
    if paragraph_is_image_only(&p) {
        p.space_after = IMAGE_AFTER_HU;
    }
    if quote {
        apply_letter_quote(&mut p);
    }
    if code {
        apply_letter_code_block(&mut p);
    }
    p
}

/// Letter CSS `code,pre{font-family:ui-monospace,Consolas,monospace}` — Consolas, not article serif.
fn apply_letter_code_font(run: &mut Run) {
    run.style.font = Some(CODE_FONT.to_string());
    run.style.latin_font = Some(CODE_FONT.to_string());
}

/// Letter CSS `pre{margin:.6rem 0 1rem}` plus the Consolas stack on the block.
fn apply_letter_code_block(p: &mut Paragraph) {
    p.code_block = true;
    p.space_before = CODE_BEFORE_HU;
    p.space_after = CODE_AFTER_HU;
    for run in &mut p.runs {
        apply_letter_code_font(run);
    }
}

/// Letter CSS `body,article{color:#134e4a}` — teal body type, not UA black.
fn apply_letter_body_color(p: &mut Paragraph) {
    for run in &mut p.runs {
        if matches!(run.content, RunContent::Text(_)) && run.style.color == Color::BLACK {
            run.style.color = BODY_FG;
        }
    }
}

/// Letter CSS `blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem;border-left:4px solid #134e4a;color:#134e4a}`.
fn apply_letter_quote(p: &mut Paragraph) {
    p.quote = true;
    p.space_before = QUOTE_BEFORE_HU;
    p.space_after = QUOTE_AFTER_HU;
    for run in &mut p.runs {
        if matches!(run.content, RunContent::Text(_)) && run.style.color == Color::BLACK {
            run.style.color = QUOTE_FG;
        }
    }
}

fn apply_letter_type(p: &mut Paragraph) {
    match p.outline_level {
        Some(1) => apply_heading_type(p, H1_SIZE_HU),
        Some(2) => apply_heading_type(p, H2_SIZE_HU),
        Some(_) => {
            p.line_spacing = LineSpacing::Percent(HEADING_LEADING_PCT);
        }
        None => {
            p.line_spacing = LineSpacing::Percent(BODY_LEADING_PCT);
        }
    }
}

fn apply_heading_type(p: &mut Paragraph, size_hu: i32) {
    p.line_spacing = LineSpacing::Percent(HEADING_LEADING_PCT);
    for run in &mut p.runs {
        run.style.size = size_hu;
        run.style.bold = true;
    }
}

fn image_para(img: ImageData) -> Paragraph {
    let mut p = Paragraph::from_text("");
    p.runs = vec![image_run(img)];
    p.line_spacing = LineSpacing::Percent(BODY_LEADING_PCT);
    p.space_after = IMAGE_AFTER_HU;
    p
}

fn image_run(img: ImageData) -> Run {
    Run {
        style: docagent_model::CharStyle::default(),
        content: RunContent::Inline(InlineObject::Image(img)),
    }
}

fn text_para(t: &str) -> Option<Paragraph> {
    let text = unescape(t);
    if text.trim().is_empty() {
        None
    } else {
        let mut p = Paragraph::from_text(text);
        p.line_spacing = LineSpacing::Percent(BODY_LEADING_PCT);
        apply_letter_body_color(&mut p);
        Some(p)
    }
}

fn aside_blocks(inner: Vec<Block>) -> Vec<Block> {
    if let [Block::Paragraph(p)] = inner.as_slice()
        && let [
            Run {
                content: RunContent::Inline(InlineObject::Image(img)),
                ..
            },
        ] = p.runs.as_slice()
    {
        return vec![Block::Float(Float::Image(img.clone()))];
    }
    inner
}

fn list_fmt(attrs: &[(String, String)]) -> NumberFormat {
    if attr(attrs, "class").is_some_and(|c| c.split_whitespace().any(|p| p == "tasks")) {
        NumberFormat::Task
    } else {
        NumberFormat::Bullet
    }
}

struct InlineOut {
    runs: Vec<Run>,
    nested: Vec<Block>,
    checked: bool,
}

struct InlineState {
    runs: Vec<Run>,
    buf: String,
    style: docagent_model::CharStyle,
}

impl InlineState {
    fn new() -> Self {
        Self {
            runs: Vec::new(),
            buf: String::new(),
            style: docagent_model::CharStyle::default(),
        }
    }

    fn flush(&mut self) {
        if self.buf.is_empty() {
            return;
        }
        let mut run = Run::text(std::mem::take(&mut self.buf));
        run.style = self.style.clone();
        self.runs.push(run);
    }

    fn apply_open(&mut self, name: &str) {
        self.flush();
        match name {
            "strong" | "b" => self.style.bold = true,
            "em" | "i" => self.style.italic = true,
            "u" => self.style.underline = docagent_model::Underline::Single,
            "s" | "del" | "strike" => self.style.strike = true,
            "code" | "kbd" | "samp" => {
                self.style.code = true;
                self.style.font = Some(CODE_FONT.to_string());
                self.style.latin_font = Some(CODE_FONT.to_string());
            }
            "mark" => self.style.highlight = Some(docagent_model::Color::MARK),
            "sup" => self.style.superscript = true,
            "sub" => self.style.subscript = true,
            _ => {}
        }
    }

    fn apply_close(&mut self, name: &str) {
        self.flush();
        match name {
            "strong" | "b" => self.style.bold = false,
            "em" | "i" => self.style.italic = false,
            "u" => self.style.underline = docagent_model::Underline::None,
            "s" | "del" | "strike" => self.style.strike = false,
            "code" | "kbd" | "samp" => {
                self.style.code = false;
                self.style.font = None;
                self.style.latin_font = None;
            }
            "mark" => self.style.highlight = None,
            "sup" => self.style.superscript = false,
            "sub" => self.style.subscript = false,
            _ => {}
        }
    }
}

fn parse_inlines(cur: &mut Cursor<'_>, until: &str) -> InlineOut {
    parse_inlines_at(cur, until, 0)
}

fn parse_inlines_at(cur: &mut Cursor<'_>, until: &str, list_level: u8) -> InlineOut {
    let mut st = InlineState::new();
    let mut nested = Vec::new();
    let mut checked = false;
    loop {
        match cur.next() {
            None => break,
            Some(Token::Close { name }) if name == until => break,
            Some(Token::Open {
                name,
                attrs,
                self_closing,
            }) => match name.as_str() {
                "img" => {
                    st.flush();
                    if let Some(img) = image_from_attrs(&attrs) {
                        st.runs.push(image_run(img));
                    }
                }
                "br" => st.buf.push('\n'),
                "a" => {
                    st.flush();
                    let href = attr(&attrs, "href").unwrap_or("");
                    let inner = if self_closing {
                        InlineOut {
                            runs: Vec::new(),
                            nested: Vec::new(),
                            checked: false,
                        }
                    } else {
                        parse_inlines_at(cur, "a", list_level)
                    };
                    let display: String = inner.runs.iter().map(Run::display_text).collect();
                    if !display.is_empty() && !href.is_empty() {
                        st.runs.push(Run::hyperlink(display, href));
                    } else {
                        st.runs.extend(inner.runs);
                    }
                    nested.extend(inner.nested);
                }
                "input" => {
                    if attr(&attrs, "type").is_some_and(|t| t.eq_ignore_ascii_case("checkbox")) {
                        checked = attrs.iter().any(|(k, _)| k == "checked");
                    }
                }
                "ul" | "ol" => {
                    st.flush();
                    let fmt = if name == "ol" {
                        NumberFormat::Decimal
                    } else {
                        list_fmt(&attrs)
                    };
                    let start = attr(&attrs, "start")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(1);
                    nested.extend(parse_list(cur, fmt, start, list_level.saturating_add(1)));
                }
                "li" if until != "li" => {
                    st.flush();
                    nested.extend(parse_li_blocks(cur, list_fmt(&[]), 1, list_level));
                }
                _ => {
                    if !self_closing && !is_void(&name) {
                        st.apply_open(&name);
                    }
                }
            },
            Some(Token::Close { name }) => st.apply_close(&name),
            Some(Token::Text(t)) => st.buf.push_str(&unescape(t)),
        }
    }
    st.flush();
    InlineOut {
        runs: st.runs,
        nested,
        checked,
    }
}

fn parse_list(cur: &mut Cursor<'_>, fmt: NumberFormat, mut start: u32, level: u8) -> Vec<Block> {
    let until = if fmt == NumberFormat::Decimal {
        "ol"
    } else {
        "ul"
    };
    let mut blocks = Vec::new();
    loop {
        match cur.next() {
            None => break,
            Some(Token::Close { name }) if name == until => break,
            Some(Token::Open { name, .. }) if name == "li" => {
                blocks.extend(parse_li_blocks(cur, fmt, start, level));
                start = start.saturating_add(1);
            }
            Some(Token::Open {
                name,
                attrs,
                self_closing: _,
            }) if name == "ul" || name == "ol" => {
                let nfmt = if name == "ol" {
                    NumberFormat::Decimal
                } else {
                    list_fmt(&attrs)
                };
                let st = attr(&attrs, "start")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);
                blocks.extend(parse_list(cur, nfmt, st, level.saturating_add(1)));
            }
            Some(Token::Open {
                name, self_closing, ..
            }) => {
                if !self_closing && !is_void(&name) && name != until {
                    let _ = parse_blocks(cur, &name);
                }
            }
            Some(Token::Close { .. } | Token::Text(_)) => {}
        }
    }
    blocks
}

fn parse_li_blocks(cur: &mut Cursor<'_>, fmt: NumberFormat, start: u32, level: u8) -> Vec<Block> {
    let inner = parse_inlines_at(cur, "li", level);
    let mut p = paragraph_from_runs(inner.runs, None, false, false);
    p.indent_left = LIST_INDENT_HU.saturating_mul(i32::from(level) + 1);
    p.space_after = LIST_ITEM_AFTER_HU;
    p.numbering = Some(NumberingRef {
        definition_id: match fmt {
            NumberFormat::Bullet => 0,
            NumberFormat::Decimal => 1,
            NumberFormat::Task => 2,
            _ => 0,
        },
        level,
        start: match fmt {
            NumberFormat::Decimal => Some(start),
            NumberFormat::Task => inner.checked.then_some(1),
            _ => None,
        },
        format: Some(fmt),
    });
    let mut blocks = vec![Block::Paragraph(p)];
    blocks.extend(inner.nested);
    blocks
}

fn parse_table(cur: &mut Cursor<'_>) -> Table {
    let mut rows = Vec::new();
    loop {
        match cur.next() {
            None => break,
            Some(Token::Close { name }) if name == "table" => break,
            Some(Token::Open {
                name, self_closing, ..
            }) if name == "tr" => {
                if !self_closing {
                    rows.push(parse_tr(cur, false));
                }
            }
            Some(Token::Open {
                name, self_closing, ..
            }) if matches!(name.as_str(), "thead" | "tbody" | "tfoot") => {
                if !self_closing {
                    parse_table_rows(cur, &name, &mut rows, name == "thead");
                }
            }
            Some(Token::Open {
                name, self_closing, ..
            }) => {
                if !self_closing && !is_void(&name) && name != "tr" {
                    let _ = parse_blocks(cur, &name);
                }
            }
            Some(Token::Close { .. } | Token::Text(_)) => {}
        }
    }
    let header_row_count = rows.iter().take_while(|r| r.header).count() as u8;
    Table {
        rows,
        borders: docagent_model::TableBorders::default(),
        split_policy: docagent_model::SplitPolicy::Allow,
        width: None,
        alignment: docagent_model::Alignment::Start,
        cell_spacing: 0,
        indent: 0,
        header_row_count,
        layout_hints: Vec::new(),
    }
}

fn parse_table_rows(
    cur: &mut Cursor<'_>,
    until: &str,
    rows: &mut Vec<TableRow>,
    header_section: bool,
) {
    loop {
        match cur.next() {
            None => break,
            Some(Token::Close { name }) if name == until => break,
            Some(Token::Open {
                name, self_closing, ..
            }) if name == "tr" => {
                if !self_closing {
                    rows.push(parse_tr(cur, header_section));
                }
            }
            Some(Token::Open {
                name, self_closing, ..
            }) => {
                if !self_closing && !is_void(&name) {
                    let _ = parse_blocks(cur, &name);
                }
            }
            Some(Token::Close { .. } | Token::Text(_)) => {}
        }
    }
}

fn parse_tr(cur: &mut Cursor<'_>, header_section: bool) -> TableRow {
    let mut cells = Vec::new();
    let mut header = header_section;
    loop {
        match cur.next() {
            None => break,
            Some(Token::Close { name }) if name == "tr" => break,
            Some(Token::Open {
                name, self_closing, ..
            }) if name == "th" || name == "td" => {
                if name == "th" {
                    header = true;
                }
                let p = para_until(cur, &name, None, false, false, self_closing);
                let mut cell = TableCell {
                    blocks: vec![Block::Paragraph(p)],
                    padding: letter_cell_padding(),
                    ..TableCell::default()
                };
                if name == "th" || header_section {
                    apply_letter_th_cell(&mut cell);
                }
                cells.push(cell);
            }
            Some(Token::Open {
                name, self_closing, ..
            }) => {
                if !self_closing && !is_void(&name) {
                    let _ = parse_blocks(cur, &name);
                }
            }
            Some(Token::Close { .. } | Token::Text(_)) => {}
        }
    }
    TableRow {
        cells,
        height: None,
        header,
        cant_split: None,
    }
}

fn image_from_attrs(attrs: &[(String, String)]) -> Option<ImageData> {
    let src = attr(attrs, "src")?;
    if src.starts_with("http://") || src.starts_with("https://") || src.starts_with("//") {
        return None;
    }
    let (mime, bytes) = decode_data_uri(src).or_else(|| read_relative_image(src))?;
    let (rw, rh) = raster_hu(&bytes);
    let style = attr(attrs, "style").unwrap_or("");
    let width = style_len(style, "width")
        .or_else(|| attr(attrs, "width").and_then(parse_css_len_hu))
        .unwrap_or(rw);
    let height = style_len(style, "height")
        .or_else(|| attr(attrs, "height").and_then(parse_css_len_hu))
        .unwrap_or(rh);
    let (width, height) = min_on_page_size(width, height);
    let alt = attr(attrs, "alt")
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Some(ImageData {
        bytes,
        mime,
        width,
        height,
        alt_text: alt,
        wrap: WrapMode::Inline,
    })
}

/// CSS/HTML length → HU. `24pt` matches PDF; unitless/`px` is 96dpi.
fn parse_css_len_hu(raw: &str) -> Option<i32> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(n) = raw.strip_suffix("pt").or_else(|| raw.strip_suffix("PT")) {
        let pt: i32 = n.trim().parse().ok()?;
        return (pt > 0).then_some(pt.saturating_mul(HU_PER_POINT));
    }
    let px_raw = raw
        .strip_suffix("px")
        .or_else(|| raw.strip_suffix("PX"))
        .unwrap_or(raw);
    let px: u32 = px_raw.trim().parse().ok()?;
    (px > 0).then(|| px_to_hu(px))
}

fn style_len(style: &str, prop: &str) -> Option<i32> {
    for part in style.split(';') {
        let Some((k, v)) = part.split_once(':') else {
            continue;
        };
        if k.trim().eq_ignore_ascii_case(prop) {
            return parse_css_len_hu(v);
        }
    }
    None
}

fn is_remote_src(src: &str) -> bool {
    let src = src.trim();
    src.starts_with("http://")
        || src.starts_with("https://")
        || src.starts_with("//")
        || src.starts_with("data:")
}

fn read_relative_image(src: &str) -> Option<(String, Vec<u8>)> {
    if is_remote_src(src) {
        return None;
    }
    let bytes = read_relative_bytes(src)?;
    Some((mime_from_name(src), bytes))
}

fn mime_from_name(name: &str) -> String {
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e)
        .unwrap_or("")
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => "image/png",
    }
    .to_string()
}

fn read_relative_bytes(src: &str) -> Option<Vec<u8>> {
    let src = src.trim();
    if src.is_empty() {
        return None;
    }
    let path = std::path::Path::new(src);
    if path.is_absolute() {
        return std::fs::read(path).ok();
    }
    let mut roots = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd);
    }
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if !roots.iter().any(|r| r == &manifest) {
        roots.push(manifest);
    }
    for root in roots {
        let mut dir = root.as_path();
        loop {
            if let Ok(bytes) = std::fs::read(dir.join(path)) {
                return Some(bytes);
            }
            match dir.parent() {
                Some(parent) => dir = parent,
                None => break,
            }
        }
    }
    None
}

fn decode_data_uri(src: &str) -> Option<(String, Vec<u8>)> {
    let rest = src.strip_prefix("data:")?;
    let (meta, payload) = rest.split_once(',')?;
    let mut mime = "application/octet-stream";
    let mut is_b64 = false;
    for (i, part) in meta.split(';').enumerate() {
        if i == 0 && !part.is_empty() && !part.eq_ignore_ascii_case("base64") {
            mime = part;
        } else if part.eq_ignore_ascii_case("base64") {
            is_b64 = true;
        }
    }
    if !is_b64 {
        return None;
    }
    Some((mime.to_string(), b64_decode(payload)?))
}

fn png_ihdr_px(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.len() < 24 || !bytes.starts_with(SIG) || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    (w > 0 && h > 0).then_some((w, h))
}

fn px_to_hu(px: u32) -> i32 {
    i32::try_from(i64::from(px).saturating_mul(i64::from(HU_PER_INCH)) / 96).unwrap_or(i32::MAX)
}

/// Raise `width`×`height` so both edges are at least [`MIN_IMAGE_EDGE_HU`].
fn min_on_page_size(width: i32, height: i32) -> (i32, i32) {
    let short = width.min(height);
    if short >= MIN_IMAGE_EDGE_HU || short <= 0 {
        return (width.max(0), height.max(0));
    }
    let w = (i64::from(width) * i64::from(MIN_IMAGE_EDGE_HU) / i64::from(short)).max(1);
    let h = (i64::from(height) * i64::from(MIN_IMAGE_EDGE_HU) / i64::from(short)).max(1);
    (
        i32::try_from(w).unwrap_or(i32::MAX),
        i32::try_from(h).unwrap_or(i32::MAX),
    )
}

fn raster_hu(bytes: &[u8]) -> (i32, i32) {
    match png_ihdr_px(bytes) {
        Some((w, h)) => min_on_page_size(px_to_hu(w), px_to_hu(h)),
        None => (HU_PER_INCH, HU_PER_INCH),
    }
}

fn hu_to_pt(hu: i32) -> i32 {
    hu.div_euclid(HU_PER_POINT).max(MIN_IMAGE_EDGE_PT)
}

/// On-page size WeasyPrint must paint: IR, else raster, then the 24pt floor.
fn img_display_hu(img: &ImageData) -> (i32, i32) {
    let (w, h) = if img.width > 0 && img.height > 0 {
        (img.width, img.height)
    } else if img.width > 0 {
        (img.width, img.width)
    } else if img.height > 0 {
        (img.height, img.height)
    } else {
        raster_hu(&img.bytes)
    };
    min_on_page_size(w, h)
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
        docagent_model::NumberFormat::Task => s.push_str(r#"<ul class="tasks">"#),
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
        if fmt == docagent_model::NumberFormat::Task {
            if p.numbering.as_ref().is_some_and(|n| n.start == Some(1)) {
                s.push_str(r#"<input type="checkbox" disabled="" checked=""/>"#);
            } else {
                s.push_str(r#"<input type="checkbox" disabled=""/>"#);
            }
        }
        push_runs(s, p);
        *i += 1;
        if *i < blocks.len() && list_level(&blocks[*i]).is_some_and(|nl| nl > base) {
            emit_list(s, blocks, i);
        }
        s.push_str("</li>");
    }
    match fmt {
        docagent_model::NumberFormat::Bullet | docagent_model::NumberFormat::Task => {
            s.push_str("</ul>")
        }
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
        Block::Float(docagent_model::Float::Image(img)) => {
            s.push_str("<aside>");
            push_img(s, img);
            s.push_str("</aside>");
        }
        Block::Float(_) => s.push_str("<aside></aside>"),
        Block::Break(_) => s.push_str("<hr/>"),
    }
}

fn push_para(s: &mut String, p: &Paragraph) {
    if p.code_block {
        s.push_str("<pre><code>");
        s.push_str(&escape(&p.plain_text()));
        s.push_str("</code></pre>");
        return;
    }
    if p.quote {
        s.push_str("<blockquote>");
        push_runs(s, p);
        s.push_str("</blockquote>");
        return;
    }
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
        match &run.content {
            RunContent::Inline(docagent_model::InlineObject::Image(img)) => push_img(s, img),
            RunContent::Inline(docagent_model::InlineObject::Hyperlink { target, display }) => {
                if display.is_empty() {
                    continue;
                }
                s.push_str("<a href=\"");
                s.push_str(&escape(target));
                s.push_str("\">");
                s.push_str(&escape(display));
                s.push_str("</a>");
            }
            RunContent::Text(t) => {
                if t.is_empty() {
                    continue;
                }
                let escaped = escape(t);
                let under = run.style.underline != docagent_model::Underline::None;
                if run.style.bold {
                    s.push_str("<strong>");
                }
                if run.style.italic {
                    s.push_str("<em>");
                }
                if under {
                    s.push_str("<u>");
                }
                if run.style.strike {
                    s.push_str("<s>");
                }
                if run.style.code {
                    s.push_str("<code>");
                }
                if run.style.highlight.is_some() {
                    s.push_str("<mark>");
                }
                if run.style.superscript {
                    s.push_str("<sup>");
                }
                if run.style.subscript {
                    s.push_str("<sub>");
                }
                s.push_str(&escaped);
                if run.style.subscript {
                    s.push_str("</sub>");
                }
                if run.style.superscript {
                    s.push_str("</sup>");
                }
                if run.style.highlight.is_some() {
                    s.push_str("</mark>");
                }
                if run.style.code {
                    s.push_str("</code>");
                }
                if run.style.strike {
                    s.push_str("</s>");
                }
                if under {
                    s.push_str("</u>");
                }
                if run.style.italic {
                    s.push_str("</em>");
                }
                if run.style.bold {
                    s.push_str("</strong>");
                }
            }
            RunContent::Inline(_) => {}
        }
    }
}

fn push_table(s: &mut String, t: &Table) {
    s.push_str("<table>");
    let header_n = t
        .rows
        .iter()
        .enumerate()
        .take_while(|(ri, row)| row.header || *ri < t.header_row_count as usize)
        .count();
    if header_n > 0 {
        s.push_str("<thead>");
        for row in t.rows.iter().take(header_n) {
            push_tr(s, row, true);
        }
        s.push_str("</thead>");
    }
    if header_n < t.rows.len() {
        s.push_str("<tbody>");
        for row in t.rows.iter().skip(header_n) {
            push_tr(s, row, false);
        }
        s.push_str("</tbody>");
    }
    s.push_str("</table>");
}

fn push_tr(s: &mut String, row: &TableRow, header: bool) {
    s.push_str("<tr>");
    let tag = if header { "th" } else { "td" };
    for cell in &row.cells {
        s.push_str(&format!("<{tag}>"));
        for b in &cell.blocks {
            if let Block::Paragraph(p) = b {
                push_runs(s, p);
            }
        }
        s.push_str(&format!("</{tag}>"));
    }
    s.push_str("</tr>");
}

fn push_img(s: &mut String, img: &ImageData) {
    if img.bytes.is_empty() {
        return;
    }
    s.push_str("<img alt=\"");
    s.push_str(&escape_attr(img.alt_text.as_deref().unwrap_or("")));
    s.push_str("\" src=\"");
    s.push_str(&escape_attr(&image_src(img)));
    s.push('"');
    let (w, h) = img_display_hu(img);
    if w > 0 && h > 0 {
        let wp = hu_to_pt(w);
        let hp = hu_to_pt(h);
        // WeasyPrint uses PNG intrinsic px unless the tag names pt; CSS min-* is not enough.
        s.push_str(&format!(" style=\"width:{wp}pt;height:{hp}pt\""));
    }
    s.push_str("/>");
}

fn image_src(img: &ImageData) -> String {
    let mime = if img.mime.is_empty() {
        "image/png"
    } else {
        img.mime.as_str()
    };
    let mut s = String::from("data:");
    s.push_str(mime);
    s.push_str(";base64,");
    s.push_str(&b64_encode(&img.bytes));
    s
}

const B64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    let mut i = 0;
    while i < data.len() {
        let remaining = data.len() - i;
        let a = data[i];
        let b = if remaining > 1 { data[i + 1] } else { 0 };
        let c = if remaining > 2 { data[i + 2] } else { 0 };
        let n = (u32::from(a) << 16) | (u32::from(b) << 8) | u32::from(c);
        out.push(char::from(B64_ALPHABET[((n >> 18) & 63) as usize]));
        out.push(char::from(B64_ALPHABET[((n >> 12) & 63) as usize]));
        if remaining > 1 {
            out.push(char::from(B64_ALPHABET[((n >> 6) & 63) as usize]));
        } else {
            out.push('=');
        }
        if remaining > 2 {
            out.push(char::from(B64_ALPHABET[(n & 63) as usize]));
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

fn b64_val(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    }
}

fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let mut raw = Vec::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_whitespace() {
            continue;
        }
        raw.push(b);
    }
    while !raw.is_empty() && raw.len() % 4 != 0 {
        raw.push(b'=');
    }
    if raw.is_empty() {
        return Some(Vec::new());
    }
    let mut out = Vec::with_capacity(raw.len() / 4 * 3);
    for chunk in raw.chunks(4) {
        let a = b64_val(chunk[0])?;
        let b = b64_val(chunk[1])?;
        let c_pad = chunk[2] == b'=';
        let d_pad = chunk[3] == b'=';
        let c = if c_pad { 0 } else { b64_val(chunk[2])? };
        let d = if d_pad { 0 } else { b64_val(chunk[3])? };
        let n = (u32::from(a) << 18) | (u32::from(b) << 12) | (u32::from(c) << 6) | u32::from(d);
        out.push((n >> 16) as u8);
        if !c_pad {
            out.push((n >> 8) as u8);
        }
        if !d_pad {
            out.push(n as u8);
        }
    }
    Some(out)
}

fn escape(t: &str) -> String {
    t.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_attr(t: &str) -> String {
    escape(t).replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_has_semantic_structure() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("hello")));
        section.body.push(Block::Table(Table::from_cells(vec![vec![
            "a".into(),
            "b".into(),
        ]])));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains("<article"));
        assert!(html.contains("<section>"));
        assert!(html.contains("<p>hello</p>"));
        assert!(html.contains("<table>"));
        assert!(html.contains("<td>a</td>"));
    }

    #[test]
    fn table_headers_emit_th() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains("<th>Hop</th>"), "{html}");
        assert!(html.contains("<td>Input</td>"), "{html}");
        assert!(
            html.contains("<thead><tr><th>Hop</th><th>File</th></tr></thead>"),
            "header rows must sit in <thead>, not a first-row <tr> WeasyPrint will not repeat: {html}"
        );
        assert!(
            html.contains("<tbody><tr><td>Input</td><td>letter.md</td></tr></tbody>"),
            "body rows must sit in <tbody> under thead: {html}"
        );
        assert!(
            html.contains(THEAD_RULE) && html.contains(TBODY_RULE) && html.contains(TH_RULE),
            "thead must repeat, tbody must stay row-group, and first-row th must keep the hop fill: {html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th must keep WeasyPrint teal fill + header rule, not a sparse grid: {html}"
        );
        assert!(
            html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem"),
            "cell pad/border must not regress: {html}"
        );
        assert!(
            !html.contains("th{background:transparent")
                && !html.contains("th{background:none")
                && !html.contains("th{background:#fff")
                && !html.contains("th{background:white")
                && !html.contains("th{font-weight:400")
                && !html.contains("th{border:none")
                && !html.contains("th{border-bottom:1px"),
            "UA/reset th is a sparse grid, not WeasyPrint letter input: {html}"
        );
    }

    #[test]
    fn read_roundtrips_table_header() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains("<th>Hop</th>"), "{html}");
        assert!(html.contains("<td>Input</td>"), "{html}");
        let back = read(html.as_bytes()).expect("html");
        let t = first_table(&back);
        assert!(t.rows[0].header);
        assert!(!t.rows[1].header);
        assert!(t.header_row_count >= 1);
        for cell in &t.rows[0].cells {
            assert_eq!(
                cell.shading,
                Some(TH_BG),
                "round-trip <th> must keep letter CSS #ccfbf1 fill: {:?}",
                cell.shading
            );
            assert_eq!(cell.padding.left, CELL_PAD_X_HU);
            assert!(cell.blocks.iter().any(|b| match b {
                Block::Paragraph(p) => {
                    p.runs
                        .iter()
                        .any(|r| r.style.bold && r.style.color == TH_FG)
                }
                _ => false,
            }));
        }
        for cell in &t.rows[1].cells {
            assert_ne!(cell.shading, Some(TH_BG), "body <td> must not keep th fill");
        }
    }

    #[test]
    fn read_honors_thead_rows() {
        let html = b"<table><thead><tr><td>Hop</td><td>File</td></tr></thead><tbody><tr><td>Input</td><td>letter.md</td></tr></tbody></table>";
        let doc = read(html).expect("html");
        let t = first_table(&doc);
        assert!(t.rows[0].header);
        assert!(!t.rows[1].header);
        assert!(t.header_row_count >= 1);
        assert_eq!(t.rows[0].cells[0].shading, Some(TH_BG));
        assert_eq!(t.rows[1].cells[0].shading, None);
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
        assert!(html.contains("h1{font-size:1.75rem"), "{html}");
        assert!(html.contains("h2{font-size:1.15rem"), "{html}");
        assert!(
            html.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{html}"
        );
        assert!(
            html.contains("h2{font-size:1.15rem;line-height:1.2"),
            "{html}"
        );
        assert!(html.contains("font-size:10pt;line-height:1.45"), "{html}");
        assert!(html.contains("border-bottom:3px solid #134e4a"));
        assert!(html.contains("border-bottom:2px solid #0d9488"));
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
        assert!(
            html.contains("<ul><li>dock<ul><li>bay 4</li></ul></li></ul>"),
            "{html}"
        );
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
        assert!(
            html.contains("strong,b{font-weight:700}"),
            "strong/b must keep letter.html weight 700, not a missing UA bolder: {html}"
        );
        assert!(
            html.contains("em,i{font-style:italic}"),
            "em/i must keep letter.html italic, not a missing UA style: {html}"
        );
        assert!(
            !html.contains("strong{font-weight:400")
                && !html.contains("strong{font-weight:normal")
                && !html.contains("strong{font-weight:bolder")
                && !html.contains("b{font-weight:400")
                && !html.contains("b{font-weight:normal")
                && !html.contains("em{font-style:normal")
                && !html.contains("i{font-style:normal"),
            "reset/normal/bolder strong/em hides letter GPU-free / byte-for-byte: {html}"
        );
        assert!(
            html.contains("sup{font-size:.75em;line-height:normal;vertical-align:super}"),
            "super must not regress: {html}"
        );
        assert!(
            html.contains("sub{font-size:.75em;line-height:normal;vertical-align:sub}"),
            "sub must not regress: {html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th fill must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "tasks must not regress: {html}"
        );
        assert!(
            html.contains("img{min-width:24pt;min-height:24pt"),
            "24pt img floor must not regress: {html}"
        );
    }

    #[test]
    fn superscripts_emit_sup() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("A");
        p.runs[0].style.superscript = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains("<sup>A</sup>"), "{html}");
        assert!(
            html.contains("sup{font-size:.75em;line-height:normal;vertical-align:super}"),
            "super must keep WeasyPrint 0.75em size, normal leading, and super offset, not a missing UA style: {html}"
        );
        assert!(
            !html.contains("sup{vertical-align:baseline")
                && !html.contains("sup{font-size:1em")
                && !html.contains("sup{font-size:inherit")
                && !html.contains("sup{line-height:1.45"),
            "reset/inherited super hides letter PDF/A: {html}"
        );
        assert!(
            html.contains("s,del{text-decoration:line-through;color:#134e4a}"),
            "strike must not regress: {html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th fill must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "tasks must not regress: {html}"
        );
        assert!(
            html.contains("img{min-width:24pt;min-height:24pt"),
            "24pt img floor must not regress: {html}"
        );
    }

    #[test]
    fn subscripts_emit_sub() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("2");
        p.runs[0].style.subscript = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains("<sub>2</sub>"), "{html}");
        assert!(
            html.contains("sub{font-size:.75em;line-height:normal;vertical-align:sub}"),
            "sub must keep WeasyPrint 0.75em size, normal leading, and sub offset, not a missing UA style: {html}"
        );
        assert!(
            !html.contains("sub{vertical-align:baseline")
                && !html.contains("sub{font-size:1em")
                && !html.contains("sub{font-size:inherit")
                && !html.contains("sub{line-height:1.45"),
            "reset/inherited sub hides letter H2O: {html}"
        );
        assert!(
            html.contains("s,del{text-decoration:line-through;color:#134e4a}"),
            "strike must not regress: {html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th fill must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "tasks must not regress: {html}"
        );
        assert!(
            html.contains("img{min-width:24pt;min-height:24pt"),
            "24pt img floor must not regress: {html}"
        );
    }

    #[test]
    fn underlines_emit_u() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("09:00 local");
        p.runs[0].style.underline = docagent_model::Underline::Single;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains("<u>09:00 local</u>"), "{html}");
        assert!(
            html.contains("u{text-decoration:underline}"),
            "underline runs must keep WeasyPrint <u> decoration, not a missing UA style: {html}"
        );
        assert!(
            !html.contains("u{text-decoration:none")
                && !html.contains("u{text-decoration:line-through"),
            "reset <u> hides letter underlines: {html}"
        );
    }

    #[test]
    fn highlights_emit_mark() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("hashes");
        p.runs[0].style.highlight = Some(docagent_model::Color::MARK);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains("<mark>hashes</mark>"), "{html}");
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "highlights must stay letter.html amber + body teal, not UA yellow/black: {html}"
        );
        assert!(
            !html.contains("mark{background:transparent")
                && !html.contains("mark{background:none")
                && !html.contains("mark{background:yellow")
                && !html.contains("mark{color:black")
                && !html.contains("mark{color:#000")
                && !html.contains("mark{background:yellow;color:black"),
            "UA yellow/black mark punches the letter teal chip: {html}"
        );
    }

    #[test]
    fn tasks_emit_checkboxes() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut a = Paragraph::from_text("Replay the convert");
        a.numbering = Some(docagent_model::NumberingRef {
            definition_id: 2,
            level: 0,
            start: Some(1),
            format: Some(docagent_model::NumberFormat::Task),
        });
        let mut b = Paragraph::from_text("Hope");
        b.numbering = Some(docagent_model::NumberingRef {
            definition_id: 2,
            level: 0,
            start: None,
            format: Some(docagent_model::NumberFormat::Task),
        });
        section.body.push(Block::Paragraph(a));
        section.body.push(Block::Paragraph(b));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains(r#"<ul class="tasks">"#), "{html}");
        assert!(
            html.contains(r#"<input type="checkbox" disabled="" checked=""/>Replay the convert"#),
            "{html}"
        );
        assert!(
            html.contains(r#"<input type="checkbox" disabled=""/>Hope"#),
            "{html}"
        );
        assert!(!html.contains("<p>Hope</p>"), "{html}");
        assert!(
            html.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "task lists must drop the disc so the checkbox is the marker: {html}"
        );
        assert!(
            html.contains("ul.tasks>li{list-style:none}"),
            "task items must not keep a UA disc beside the checkbox: {html}"
        );
        assert!(
            html.contains("ul.tasks input[type=checkbox]{appearance:none;-webkit-appearance:none;display:inline-block;width:.9em;height:.9em;margin-right:.4rem;vertical-align:middle;border:1.5pt solid #134e4a;border-radius:2px;background:#fff;accent-color:#134e4a}"),
            "WeasyPrint misses native widgets; task checkboxes must be CSS boxes: {html}"
        );
        assert!(
            html.contains("ul.tasks input[type=checkbox]:checked{background:#134e4a}"),
            "checked - [x] must fill teal, not look like an empty box: {html}"
        );
    }

    #[test]
    fn code_blocks_emit_pre() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("docagent prove examples/letter.md");
        p.code_block = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(
            html.contains("<pre><code>docagent prove examples/letter.md</code></pre>"),
            "{html}"
        );
        assert!(html.contains("pre{background:#ccfbf1"), "{html}");
        assert!(
            html.contains("pre{background:#ccfbf1;padding:.75rem 1rem;margin:.6rem 0 1rem;border-radius:4px;overflow:auto;font-family:ui-monospace,Consolas,monospace}"),
            "pre must use letter.html ui-monospace/Consolas, not article serif: {html}"
        );
        assert!(
            html.contains("pre code{background:transparent;padding:0;color:#134e4a;font-family:ui-monospace,Consolas,monospace}"),
            "pre code must keep the same Consolas stack as inline code: {html}"
        );
        assert!(
            !html.contains("pre{font-family:serif")
                && !html.contains(r#"pre{font-family:"Times"#)
                && !html.contains(r#"pre{font-family:"Segoe UI""#),
            "pre must not inherit article serif/sans: {html}"
        );
    }

    #[test]
    fn thematic_breaks_emit_hr() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Break(BreakKind::Thematic));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains("<hr/>"), "{html}");
        assert!(
            html.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"),
            "hr must match letter.html 3px / 1.1rem, not UA 1px inset: {html}"
        );
        assert!(
            !html.contains("hr{border:1px") && !html.contains("hr{margin:.5em"),
            "UA hr is not WeasyPrint letter input: {html}"
        );
    }

    #[test]
    fn code_spans_emit_code() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("prove");
        p.runs[0].style.code = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "inline code must keep letter.html ui-monospace/Consolas chip, not a bare UA serif span: {html}"
        );
        assert!(
            html.contains("pre code{background:transparent;padding:0;color:#134e4a;font-family:ui-monospace,Consolas,monospace}"),
            "pre code must not inherit the inline-code chip and must keep Consolas: {html}"
        );
        assert!(
            !html.contains("code{background:none")
                && !html.contains("code{padding:0;border-radius:0"),
            "reset code styles hide WeasyPrint letter chips: {html}"
        );
        assert!(
            !html.contains("code{font-family:serif")
                && !html.contains(r#"code{font-family:"Times"#)
                && !html.contains(r#"code{font-family:"Segoe UI""#),
            "inline code must not inherit article serif/sans: {html}"
        );
    }

    #[test]
    fn quotes_emit_blockquote() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Replay is evidence.");
        p.quote = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(
            html.contains("<blockquote>Replay is evidence.</blockquote>"),
            "{html}"
        );
        assert!(
            html.contains(
                "blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem;border-left:4px solid #134e4a;color:#134e4a}"
            ),
            "blockquote left bar must stay letter.html 4px #134e4a: {html}"
        );
        assert!(
            !html.contains("blockquote{border-left:none")
                && !html.contains("blockquote{border-left:0")
                && !html.contains("blockquote{border-left:1px")
                && !html.contains("blockquote{border-left:3px"),
            "UA/reset quote bar is not WeasyPrint letter input: {html}"
        );
    }

    #[test]
    fn strikethrough_emits_s() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("guess");
        p.runs[0].style.strike = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains("<s>guess</s>"), "{html}");
        assert!(
            html.contains("s,del{text-decoration:line-through;color:#134e4a}"),
            "strike runs must keep WeasyPrint teal line-through on s and del, not a missing UA style: {html}"
        );
        assert!(
            !html.contains("s{text-decoration:none")
                && !html.contains("del{text-decoration:none")
                && !html.contains("s,del{text-decoration:none"),
            "reset s/del hides letter strikethrough: {html}"
        );
    }

    #[test]
    fn hyperlinks_emit_anchor() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![docagent_model::Run::hyperlink(
            "DocAgent",
            "https://github.com/kevin9327/docagent",
        )];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(
            html.contains(r#"<a href="https://github.com/kevin9327/docagent">DocAgent</a>"#),
            "{html}"
        );
        assert!(
            html.contains(LINK_RULE) && html.contains("a{color:#0d9488;text-decoration:underline}"),
            "links must stay teal and underlined like WeasyPrint: {html}"
        );
        assert!(
            html.contains(LINK_HOVER_RULE) && html.contains("a:hover{color:#134e4a}"),
            "link hover must rest to letter teal #134e4a, not UA red/blue: {html}"
        );
        assert!(
            !html.contains("a{color:blue") && !html.contains("a{text-decoration:none"),
            "UA/reset link styles hide WeasyPrint letter links: {html}"
        );
    }

    fn fixture_png() -> Vec<u8> {
        include_bytes!("../../../docs/assets/mark.png").to_vec()
    }

    fn fixture_image(alt: &str) -> ImageData {
        ImageData {
            bytes: fixture_png(),
            mime: "image/png".into(),
            width: 0,
            height: 0,
            alt_text: Some(alt.into()),
            wrap: WrapMode::Inline,
        }
    }

    fn first_img_tag<'a>(html: &'a str, alt: &str) -> &'a str {
        let needle = format!(r#"<img alt="{alt}""#);
        let start = html
            .find(&needle)
            .unwrap_or_else(|| panic!("missing <img alt=\"{alt}\">: {html}"));
        let rest = &html[start..];
        let end = rest
            .find("/>")
            .unwrap_or_else(|| panic!("unclosed <img alt=\"{alt}\">: {html}"));
        &rest[..end + 2]
    }

    fn img_tag_display_pt(html: &str, alt: &str) -> Option<(i32, i32)> {
        let tag = first_img_tag(html, alt);
        let style = tag
            .split_once("style=\"")
            .and_then(|(_, rest)| rest.split_once('"'))?
            .0;
        let w = style_len(style, "width")?.div_euclid(HU_PER_POINT);
        let h = style_len(style, "height")?.div_euclid(HU_PER_POINT);
        Some((w, h))
    }

    fn first_image(doc: &Document) -> &ImageData {
        for section in &doc.sections {
            for block in &section.body {
                match block {
                    Block::Paragraph(p) => {
                        for run in &p.runs {
                            if let RunContent::Inline(InlineObject::Image(img)) = &run.content {
                                return img;
                            }
                        }
                    }
                    Block::Float(Float::Image(img)) => return img,
                    _ => {}
                }
            }
        }
        panic!("no image");
    }

    fn first_para(doc: &Document) -> &Paragraph {
        for section in &doc.sections {
            for block in &section.body {
                if let Block::Paragraph(p) = block {
                    return p;
                }
            }
        }
        panic!("no paragraph");
    }

    fn first_table(doc: &Document) -> &Table {
        for section in &doc.sections {
            for block in &section.body {
                if let Block::Table(t) = block {
                    return t;
                }
            }
        }
        panic!("no table");
    }

    fn numbered_paras(doc: &Document) -> Vec<&Paragraph> {
        doc.sections
            .iter()
            .flat_map(|s| &s.body)
            .filter_map(|b| match b {
                Block::Paragraph(p) if p.numbering.is_some() => Some(p),
                _ => None,
            })
            .collect()
    }

    fn list_item(text: &str, format: NumberFormat, level: u8, start: Option<u32>) -> Paragraph {
        let mut p = Paragraph::from_text(text);
        p.numbering = Some(NumberingRef {
            definition_id: match format {
                NumberFormat::Bullet => 0,
                NumberFormat::Decimal => 1,
                NumberFormat::Task => 2,
                _ => 0,
            },
            level,
            start,
            format: Some(format),
        });
        p
    }

    #[test]
    fn images_emit_img() {
        let png = fixture_png();
        assert!(!png.is_empty());
        let img = fixture_image("mark");
        assert_eq!(img.bytes, png);
        let src = image_src(&img);
        assert!(src.starts_with("data:image/png;base64,"));
        assert!(src.len() > "data:image/png;base64,".len());
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![docagent_model::Run {
            style: docagent_model::CharStyle::default(),
            content: RunContent::Inline(docagent_model::InlineObject::Image(img.clone())),
        }];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(!html.contains("<img/>"), "{html}");
        assert!(!html.contains(r#"src="""#), "{html}");
        let tag = format!(r#"<img alt="mark" src="{src}" style="width:24pt;height:24pt"/>"#);
        assert!(html.contains(&tag), "{html}");
        assert!(
            html.contains("img{min-width:24pt;min-height:24pt;max-width:100%;height:auto"),
            "{html}"
        );
        assert!(
            first_img_tag(&html, "mark").contains(r#"style="width:24pt;height:24pt""#),
            "WeasyPrint display size must sit on the <img>, not only CSS min-*: {html}"
        );
        let marker = r#"src="data:image/png;base64,"#;
        let start = html.find(marker).expect("data-uri img");
        let payload = &html[start + marker.len()..];
        let end = payload.find('"').expect("src end");
        assert!(end > 0, "empty data-uri is not a picture: {html}");
        let decoded = b64_decode(&payload[..end]).expect("b64");
        assert_eq!(decoded, img.bytes);
        assert_eq!(decoded, png);
        assert_eq!(
            decoded.as_slice(),
            include_bytes!("../../../docs/assets/mark.png")
        );
    }

    #[test]
    fn table_cells_emit_strong_and_em() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![vec!["plain".into()]]);
        let mut p = Paragraph::from_text("");
        let mut bold = docagent_model::Run::text("GPU-free");
        bold.style.bold = true;
        let mut italic = docagent_model::Run::text(" byte-for-byte");
        italic.style.italic = true;
        p.runs = vec![bold, italic];
        table.rows[0].cells[0].blocks = vec![Block::Paragraph(p)];
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(
            html.contains("<td><strong>GPU-free</strong><em> byte-for-byte</em></td>"),
            "{html}"
        );
    }

    #[test]
    fn read_roundtrips_img_data_uri() {
        let png: &[u8] = include_bytes!("../../../docs/assets/mark.png");
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![Run {
            style: docagent_model::CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(ImageData {
                bytes: png.to_vec(),
                mime: "image/png".into(),
                width: 0,
                height: 0,
                alt_text: Some("mark".into()),
                wrap: WrapMode::Inline,
            })),
        }];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(
            html.contains(r#"<img alt="mark" src="data:image/png;base64,"#),
            "{html}"
        );
        let back = read(html.as_bytes()).expect("html");
        let img = first_image(&back);
        assert_eq!(img.bytes.as_slice(), png);
        assert_eq!(img.mime, "image/png");
        assert_eq!(img.alt_text.as_deref(), Some("mark"));
        assert_eq!(img.wrap, WrapMode::Inline);
        let (w, h) = raster_hu(png);
        assert_eq!((img.width, img.height), (w, h));
        assert_ne!((img.width, img.height), (0, 0));
    }

    #[test]
    fn read_non_png_data_uri_is_one_inch() {
        let jpeg = [0xFF, 0xD8, 0xFF, 0xD9];
        let html = format!(
            r#"<p><img alt="j" src="data:image/jpeg;base64,{}"/></p>"#,
            b64_encode(&jpeg)
        );
        let doc = read(html.as_bytes()).expect("html");
        let img = first_image(&doc);
        assert_eq!(img.bytes.as_slice(), jpeg.as_slice());
        assert_eq!(img.mime, "image/jpeg");
        assert_eq!(img.alt_text.as_deref(), Some("j"));
        assert_eq!(img.width, HU_PER_INCH);
        assert_eq!(img.height, HU_PER_INCH);
        assert_eq!(img.wrap, WrapMode::Inline);
    }

    #[test]
    fn read_skips_http_img_src() {
        let html = br#"<p><img alt="x" src="https://example.com/a.png"/></p>"#;
        let doc = read(html).expect("html");
        let has_img = doc.sections.iter().any(|s| {
            s.body.iter().any(|b| match b {
                Block::Paragraph(p) => p
                    .runs
                    .iter()
                    .any(|r| matches!(r.content, RunContent::Inline(InlineObject::Image(_)))),
                Block::Float(Float::Image(_)) => true,
                _ => false,
            })
        });
        assert!(!has_img);
    }

    #[test]
    fn read_relative_img_src() {
        let png = fixture_png();
        let html = br#"<p><img alt="mark" src="docs/assets/mark.png"/></p>"#;
        let doc = read(html).expect("html");
        let img = first_image(&doc);
        assert_eq!(img.bytes, png);
        assert!(!img.bytes.is_empty());
        assert_eq!(img.mime, "image/png");
        assert_eq!(img.alt_text.as_deref(), Some("mark"));
        assert_eq!(img.wrap, WrapMode::Inline);
        let (width, height) = raster_hu(&png);
        assert_eq!((img.width, img.height), (width, height));
        assert_ne!((img.width, img.height), (0, 0));
        let out = to_html(&doc);
        assert!(
            out.contains(r#"<img alt="mark" src="data:image/png;base64,"#),
            "{out}"
        );
        assert!(
            first_img_tag(&out, "mark").contains(r#"style="width:24pt;height:24pt""#),
            "relative path must still paint at the PDF 24pt floor: {out}"
        );
        assert_eq!(
            img_tag_display_pt(&out, "mark"),
            Some((MIN_IMAGE_EDGE_PT, MIN_IMAGE_EDGE_PT)),
            "HTML <img> display must match PDF 24pt after a relative-path read: {out}"
        );
        let start = out.find(r#"src="data:image/png;base64,"#).expect("src");
        let payload = &out[start + r#"src="data:image/png;base64,"#.len()..];
        let end = payload.find('"').expect("src end");
        let decoded = b64_decode(&payload[..end]).expect("b64");
        assert_eq!(decoded, png);
    }

    #[test]
    fn letter_md_to_html_weasyprint_article_input() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("| Hop |"),
            "letter.md must contain the hop table"
        );
        assert!(
            text.contains("![Harbor mark]"),
            "examples/letter.md must contain a markdown image"
        );
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let img_p = first_image_paragraph(&doc);
        assert_eq!(
            img_p.space_after, IMAGE_AFTER_HU,
            "letter.md harbor mark IR gap {} must be CSS 0.7rem, not body 200",
            img_p.space_after
        );
        let t = first_table(&doc);
        assert_eq!(t.rows[0].cells[0].padding.left, CELL_PAD_X_HU);
        assert_eq!(t.rows[0].cells[0].padding.top, CELL_PAD_Y_HU);
        let img = first_image(&doc);
        assert!(img.width >= MIN_IMAGE_EDGE_HU);
        assert!(img.height >= MIN_IMAGE_EDGE_HU);
        let html = to_html(&doc);
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(html.contains("<th><strong>Hop</strong></th>"), "{html}");
        assert!(
            html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem"),
            "letter table must not be a sparse grid: {html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "letter table header must keep WeasyPrint teal fill, not a sparse grid: {html}"
        );
        assert!(
            html.contains("p{margin:0 0 .7rem}")
                && html.contains(
                    "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
                ),
            "letter HTML must keep the post-mark gap and 24pt floor: {html}"
        );
        assert!(html.contains("line-height:1.45"), "{html}");
        assert!(html.contains("font-size:10pt;line-height:1.45"), "{html}");
        assert!(
            html.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{html}"
        );
        assert!(
            html.contains("h2{font-size:1.15rem;line-height:1.2"),
            "{html}"
        );
        assert!(html.contains(r#"article{max-width:48rem"#), "{html}");
        assert!(
            html.contains(PAGE_RULE),
            "letter HTML print must pin @page 2.5rem 2.75rem, not WeasyPrint UA 75px: {html}"
        );
        assert!(
            html.contains(PRINT_RULE),
            "letter HTML print must not stack UA page margin on the screen card: {html}"
        );
        assert!(
            html.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"),
            "letter HTML hr must match letter.html 3px / 1.1rem: {html}"
        );
        assert!(html.contains("<hr/>"), "{html}");
        assert!(
            html.contains(LINK_RULE) && html.contains(LINK_HOVER_RULE),
            "letter HTML links must stay teal, underlined, and hover to #134e4a: {html}"
        );
        assert!(
            html.contains(r#"<a href="https://github.com/kevin9327/docagent">DocAgent</a>"#),
            "{html}"
        );
        assert!(html.contains("PDF/<sup>A</sup>"), "{html}");
        assert!(html.contains("H<sub>2</sub>O"), "{html}");
        assert!(
            html.contains("sup{font-size:.75em;line-height:normal;vertical-align:super}"),
            "letter HTML super must keep WeasyPrint size and offset: {html}"
        );
        assert!(
            html.contains("sub{font-size:.75em;line-height:normal;vertical-align:sub}"),
            "letter HTML sub must keep WeasyPrint size and offset: {html}"
        );
        assert!(
            html.contains("s,del{text-decoration:line-through;color:#134e4a}"),
            "letter HTML strike must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "letter HTML tasks must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_to_html_contains_img() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        if !text.contains("![") {
            return;
        }
        let png = fixture_png();
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert_eq!(img.bytes, png);
        assert!(!img.bytes.is_empty());
        let html = to_html(&doc);
        assert!(html.contains("<img"), "{html}");
        let marker = r#"src="data:image/png;base64,"#;
        let start = html.find(marker).expect("data-uri img");
        let payload = &html[start + marker.len()..];
        let end = payload.find('"').expect("src end");
        assert!(end > 0, "empty data-uri is not a picture: {html}");
        let decoded = b64_decode(&payload[..end]).expect("b64");
        assert_eq!(decoded, png);
        assert!(
            html.contains("img{min-width:24pt;min-height:24pt;max-width:100%;height:auto"),
            "letter.md HTML must floor the 16px harbor mark to 24pt: {html}"
        );
        assert!(
            html.contains(r#"article{max-width:48rem"#) && html.contains("line-height:1.45"),
            "article CSS must stay print-like: {html}"
        );
        assert!(
            html.contains("h1{font-size:1.75rem;line-height:1.2")
                && html.contains("h2{font-size:1.15rem;line-height:1.2")
                && html.contains("font-size:10pt;line-height:1.45"),
            "letter.md HTML type must match WeasyPrint heading size and line-height: {html}"
        );
        assert!(
            !html.contains("16px"),
            "harbor mark must not size from 16px: {html}"
        );
        assert!(
            html.contains("padding:.4rem .65rem") && html.contains("<table>"),
            "letter.md HTML table must keep WeasyPrint cell padding: {html}"
        );
        assert!(
            html.contains("p{margin:0 0 .7rem}") && html.contains("margin:.6rem 0;display:block"),
            "letter.md HTML must keep a gap after the harbor mark: {html}"
        );
    }

    #[test]
    fn min_on_page_size_raises_16px_to_24pt() {
        let native = px_to_hu(16);
        assert!(
            native < MIN_IMAGE_EDGE_HU,
            "16px at 96dpi must be below the 24pt floor ({native})"
        );
        assert_eq!(
            min_on_page_size(native, native),
            (MIN_IMAGE_EDGE_HU, MIN_IMAGE_EDGE_HU)
        );
        assert_eq!(min_on_page_size(4800, 2400), (4800, 2400));
        let png = fixture_png();
        let (px_w, px_h) = png_ihdr_px(&png).expect("mark.png IHDR");
        assert_eq!((px_w, px_h), (16, 16), "fixture is 16×16 px");
        let (w, h) = raster_hu(&png);
        assert!(w >= MIN_IMAGE_EDGE_HU);
        assert!(h >= MIN_IMAGE_EDGE_HU);
        assert!(w >= HU_PER_INCH / 4);
        assert!(h >= HU_PER_INCH / 4);
    }

    #[test]
    fn article_css_stays_print_like_with_24pt_img_floor() {
        let html = to_html(&Document::new());
        assert!(html.contains(r#"article{max-width:48rem"#), "{html}");
        assert!(html.contains("font-size:10pt;line-height:1.45"), "{html}");
        assert!(html.contains("line-height:1.45"), "{html}");
        assert!(
            html.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{html}"
        );
        assert!(
            html.contains("h2{font-size:1.15rem;line-height:1.2"),
            "{html}"
        );
        assert!(html.contains(r#"font-family:"Segoe UI""#), "{html}");
        assert!(html.contains("background:#fff"), "{html}");
        assert!(
            html.contains("img{min-width:24pt;min-height:24pt;max-width:100%;height:auto"),
            "{html}"
        );
        assert!(
            !html.contains("img{max-width:100%;height:auto;margin:.6rem 0}"),
            "{html}"
        );
        assert!(
            html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem"),
            "table cells must use WeasyPrint padding, not a UA 1px sparse grid: {html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th must keep WeasyPrint header fill, not a sparse grid: {html}"
        );
        assert!(
            html.contains("p{margin:0 0 .7rem}"),
            "post-mark gap is letter CSS p margin 0.7rem: {html}"
        );
        assert!(
            html.contains("margin:.6rem 0;display:block"),
            "img must be block so the gap after the mark is visible: {html}"
        );
    }

    #[test]
    fn letter_md_html_harbor_mark_meets_pdf_24pt_floor() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("!["),
            "examples/letter.md must contain a markdown image"
        );
        let png = fixture_png();
        let (px_w, px_h) = png_ihdr_px(&png).expect("mark.png IHDR");
        assert_eq!((px_w, px_h), (16, 16), "fixture is 16×16 px");
        assert!(
            px_to_hu(16) < MIN_IMAGE_EDGE_HU,
            "16px at 96dpi must be below the 24pt floor"
        );
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert_eq!(img.bytes, png);
        assert!(img.width >= MIN_IMAGE_EDGE_HU);
        assert!(img.height >= MIN_IMAGE_EDGE_HU);
        let html = to_html(&doc);
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(
            html.contains("img{min-width:24pt;min-height:24pt"),
            "HTML of letter.md must show the harbor mark at the PDF 24pt floor, not a 16px speck: {html}"
        );
        assert!(
            html.contains(r#"article{max-width:48rem"#) && html.contains("line-height:1.45"),
            "article CSS must stay print-like: {html}"
        );
        assert!(
            html.contains("h1{font-size:1.75rem;line-height:1.2")
                && html.contains("h2{font-size:1.15rem;line-height:1.2")
                && html.contains("font-size:10pt;line-height:1.45"),
            "letter.md HTML type must match WeasyPrint heading size and line-height: {html}"
        );
        assert!(
            html.contains("padding:.4rem .65rem"),
            "letter.md HTML table must not be a sparse grid: {html}"
        );
        assert!(
            html.contains("p{margin:0 0 .7rem}") && html.contains("margin:.6rem 0;display:block"),
            "letter.md HTML must keep a gap after the harbor mark: {html}"
        );
        assert_eq!(
            img_tag_display_pt(&html, "Harbor mark"),
            Some((MIN_IMAGE_EDGE_PT, MIN_IMAGE_EDGE_PT)),
            "letter.md <img> display size must match the PDF 24pt floor, not 16px intrinsic: {html}"
        );
        assert_eq!(
            (hu_to_pt(img.width), hu_to_pt(img.height)),
            (MIN_IMAGE_EDGE_PT, MIN_IMAGE_EDGE_PT)
        );
    }

    #[test]
    fn html_img_tag_display_size_matches_pdf_24pt_floor() {
        let png = fixture_png();
        let (px_w, px_h) = png_ihdr_px(&png).expect("mark.png IHDR");
        assert_eq!((px_w, px_h), (16, 16), "fixture is 16×16 px");
        assert!(
            px_to_hu(16) < MIN_IMAGE_EDGE_HU,
            "16px at 96dpi must be below the 24pt floor"
        );
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert_eq!(img.bytes, png);
        assert_eq!(
            (img.width, img.height),
            (MIN_IMAGE_EDGE_HU, MIN_IMAGE_EDGE_HU),
            "letter.md IR must floor the harbor mark to 24pt"
        );
        let html = to_html(&doc);
        let tag = first_img_tag(&html, "Harbor mark");
        assert!(
            tag.contains(r#"style="width:24pt;height:24pt""#),
            "WeasyPrint paints PNG intrinsic 16px unless the tag names 24pt: {tag}"
        );
        assert!(
            !tag.contains("16px")
                && !tag.contains("width:12pt")
                && !tag.contains("height:12pt")
                && !tag.contains("width:16")
                && !tag.contains("height:16"),
            "harbor mark must not size from 16px/12pt: {tag}"
        );
        assert_eq!(
            img_tag_display_pt(&html, "Harbor mark"),
            Some((24, 24)),
            "HTML <img> display must match PDF 24pt: {tag}"
        );
        let back = read(html.as_bytes()).expect("html");
        let again = first_image(&back);
        assert_eq!(again.bytes, png);
        assert_eq!(
            (again.width, again.height),
            (MIN_IMAGE_EDGE_HU, MIN_IMAGE_EDGE_HU),
            "24pt style must round-trip, not collapse to 16px: {:?}",
            (again.width, again.height)
        );
        let speck = read(
            br#"<p><img alt="Harbor mark" src="docs/assets/mark.png" style="width:12pt;height:12pt"/></p>"#,
        )
        .expect("12pt html");
        let speck_img = first_image(&speck);
        assert_eq!(
            (speck_img.width, speck_img.height),
            (MIN_IMAGE_EDGE_HU, MIN_IMAGE_EDGE_HU),
            "authored 12pt must still floor to PDF 24pt"
        );
        let px16 = read(
            br#"<p><img alt="Harbor mark" src="docs/assets/mark.png" width="16" height="16"/></p>"#,
        )
        .expect("16px html");
        let px16_img = first_image(&px16);
        assert_eq!(
            (px16_img.width, px16_img.height),
            (MIN_IMAGE_EDGE_HU, MIN_IMAGE_EDGE_HU),
            "HTML width=16 (px) must floor to 24pt, not paint a speck"
        );
        let out = to_html(&px16);
        assert_eq!(
            img_tag_display_pt(&out, "Harbor mark"),
            Some((MIN_IMAGE_EDGE_PT, MIN_IMAGE_EDGE_PT)),
            "rewritten HTML must keep the 24pt display size: {out}"
        );
        assert!(
            html.contains("img{min-width:24pt;min-height:24pt;max-width:100%;height:auto"),
            "CSS 24pt floor must not regress: {html}"
        );
    }

    #[test]
    fn article_css_table_padding_and_post_mark_gap() {
        let html = to_html(&Document::new());
        assert!(
            html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem;text-align:left;font-size:.95rem}"),
            "{html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th must keep WeasyPrint teal fill + 2px header rule: {html}"
        );
        assert!(
            !html.contains("th,td{border:1px solid #cbd5e1;padding:0"),
            "zero cell padding is a sparse grid: {html}"
        );
        assert!(html.contains("p{margin:0 0 .7rem}"), "{html}");
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "{html}"
        );
        assert!(html.contains("line-height:1.45"), "{html}");
        assert!(html.contains("font-size:10pt;line-height:1.45"), "{html}");
        assert!(
            html.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{html}"
        );
        assert!(
            html.contains("h2{font-size:1.15rem;line-height:1.2"),
            "{html}"
        );
        assert!(html.contains(r#"article{max-width:48rem"#), "{html}");
    }

    #[test]
    fn html_read_applies_letter_cell_padding() {
        let html = b"<table><tr><th>Hop</th><th>File</th></tr><tr><td>Input</td><td>letter.md</td></tr></table>";
        let doc = read(html).expect("html");
        let t = first_table(&doc);
        assert!(!t.rows.is_empty());
        for row in &t.rows {
            for cell in &row.cells {
                assert_eq!(
                    cell.padding.left, CELL_PAD_X_HU,
                    "cell pad x {} must be letter CSS 0.65rem, not UA 1px",
                    cell.padding.left
                );
                assert_eq!(cell.padding.right, CELL_PAD_X_HU);
                assert_eq!(
                    cell.padding.top, CELL_PAD_Y_HU,
                    "cell pad y {} must be letter CSS 0.4rem, not UA 1px",
                    cell.padding.top
                );
                assert_eq!(cell.padding.bottom, CELL_PAD_Y_HU);
            }
        }
        assert_eq!(t.rows[0].cells[0].shading, Some(TH_BG));
        assert_eq!(t.rows[1].cells[0].shading, None);
    }

    #[test]
    fn html_read_image_para_gets_post_mark_gap() {
        let html =
            br#"<p><img alt="Harbor mark" src="docs/assets/mark.png"/></p><p>6 September 2026</p>"#;
        let doc = read(html).expect("html");
        let img_p = first_image_paragraph(&doc);
        assert!(paragraph_is_image_only(img_p));
        assert_eq!(
            img_p.space_after, IMAGE_AFTER_HU,
            "gap after mark {} must be letter CSS 0.7rem, not IR space_after 200",
            img_p.space_after
        );
        assert!(img_p.space_after > 200);
        let img = first_image(&doc);
        assert!(img.width >= MIN_IMAGE_EDGE_HU);
        assert!(img.height >= MIN_IMAGE_EDGE_HU);
        assert_eq!(img.alt_text.as_deref(), Some("Harbor mark"));
    }

    fn first_image_paragraph(doc: &Document) -> &Paragraph {
        for section in &doc.sections {
            for block in &section.body {
                if let Block::Paragraph(p) = block
                    && p.runs
                        .iter()
                        .any(|r| matches!(&r.content, RunContent::Inline(InlineObject::Image(_))))
                {
                    return p;
                }
            }
        }
        panic!("no image paragraph");
    }

    #[test]
    fn read_roundtrips_blockquote() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Replay is evidence.");
        p.quote = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(
            html.contains("<blockquote>Replay is evidence.</blockquote>"),
            "{html}"
        );
        let back = read(html.as_bytes()).expect("html");
        let p = first_para(&back);
        assert!(p.quote);
        assert!(!p.code_block);
        assert_eq!(p.plain_text(), "Replay is evidence.");
    }

    #[test]
    fn read_roundtrips_pre() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("docagent prove examples/letter.md");
        p.code_block = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(
            html.contains("<pre><code>docagent prove examples/letter.md</code></pre>"),
            "{html}"
        );
        let back = read(html.as_bytes()).expect("html");
        let p = first_para(&back);
        assert!(p.code_block);
        assert!(!p.quote);
        assert_eq!(p.plain_text(), "docagent prove examples/letter.md");
    }

    #[test]
    fn read_roundtrips_hyperlink() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![Run::hyperlink(
            "DocAgent",
            "https://github.com/kevin9327/docagent",
        )];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(
            html.contains(r#"<a href="https://github.com/kevin9327/docagent">DocAgent</a>"#),
            "{html}"
        );
        assert!(
            html.contains("a{color:#0d9488;text-decoration:underline}"),
            "round-trip HTML must keep WeasyPrint link color/underline: {html}"
        );
        let back = read(html.as_bytes()).expect("html");
        let p = first_para(&back);
        assert!(p.runs.iter().any(|r| matches!(
            &r.content,
            RunContent::Inline(InlineObject::Hyperlink { target, display })
                if target == "https://github.com/kevin9327/docagent" && display == "DocAgent"
        )));
        assert!(
            p.runs.iter().any(|r| {
                r.style.underline == docagent_model::Underline::Single
                    && matches!(
                        &r.content,
                        RunContent::Inline(InlineObject::Hyperlink { .. })
                    )
            }),
            "hyperlink IR must keep the WeasyPrint underline"
        );
    }

    #[test]
    fn read_roundtrips_mark() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("hashes");
        p.runs[0].style.highlight = Some(docagent_model::Color::MARK);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains("<mark>hashes</mark>"), "{html}");
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "round-trip HTML must keep WeasyPrint mark background: {html}"
        );
        let back = read(html.as_bytes()).expect("html");
        let p = first_para(&back);
        assert!(p.runs.iter().any(|r| {
            r.style.highlight == Some(docagent_model::Color::MARK)
                && matches!(&r.content, RunContent::Text(t) if t == "hashes")
        }));
    }

    #[test]
    fn read_roundtrips_code() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("prove");
        p.runs[0].style.code = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em"),
            "round-trip HTML must keep WeasyPrint inline-code padding: {html}"
        );
        let back = read(html.as_bytes()).expect("html");
        let p = first_para(&back);
        assert!(p.runs.iter().any(|r| {
            r.style.code && matches!(&r.content, RunContent::Text(t) if t == "prove")
        }));
        assert!(!p.code_block);
    }

    #[test]
    fn read_roundtrips_strike() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("guess");
        p.runs[0].style.strike = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains("<s>guess</s>"), "{html}");
        assert!(
            html.contains("s,del{text-decoration:line-through;color:#134e4a}"),
            "round-trip HTML must keep WeasyPrint s/del strikethrough: {html}"
        );
        let back = read(html.as_bytes()).expect("html");
        let p = first_para(&back);
        assert!(p.runs.iter().any(|r| {
            r.style.strike && matches!(&r.content, RunContent::Text(t) if t == "guess")
        }));
    }

    #[test]
    fn read_roundtrips_underline() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("09:00 local");
        p.runs[0].style.underline = docagent_model::Underline::Single;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains("<u>09:00 local</u>"), "{html}");
        assert!(
            html.contains("u{text-decoration:underline}"),
            "round-trip HTML must keep WeasyPrint <u> underline: {html}"
        );
        let back = read(html.as_bytes()).expect("html");
        let p = first_para(&back);
        assert!(p.runs.iter().any(|r| {
            r.style.underline == docagent_model::Underline::Single
                && matches!(&r.content, RunContent::Text(t) if t == "09:00 local")
        }));
    }

    #[test]
    fn read_roundtrips_superscript() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("A");
        p.runs[0].style.superscript = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains("<sup>A</sup>"), "{html}");
        assert!(
            html.contains("sup{font-size:.75em;line-height:normal;vertical-align:super}"),
            "round-trip HTML must keep WeasyPrint super size and offset: {html}"
        );
        let back = read(html.as_bytes()).expect("html");
        let p = first_para(&back);
        assert!(p.runs.iter().any(|r| {
            r.style.superscript && matches!(&r.content, RunContent::Text(t) if t == "A")
        }));
    }

    #[test]
    fn read_roundtrips_subscript() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("2");
        p.runs[0].style.subscript = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains("<sub>2</sub>"), "{html}");
        assert!(
            html.contains("sub{font-size:.75em;line-height:normal;vertical-align:sub}"),
            "round-trip HTML must keep WeasyPrint sub size and offset: {html}"
        );
        let back = read(html.as_bytes()).expect("html");
        let p = first_para(&back);
        assert!(p.runs.iter().any(|r| {
            r.style.subscript && matches!(&r.content, RunContent::Text(t) if t == "2")
        }));
    }

    #[test]
    fn read_roundtrips_header_footer() {
        let mut doc = Document::new();
        let mut section = Section {
            header: Some(header_footer(vec![Block::Paragraph(Paragraph::from_text(
                "Harbor",
            ))])),
            footer: Some(header_footer(vec![Block::Paragraph(Paragraph::from_text(
                "Page",
            ))])),
            ..Section::default()
        };
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("body")));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains("<header><p>Harbor</p></header>"), "{html}");
        assert!(html.contains("<footer><p>Page</p></footer>"), "{html}");
        let back = read(html.as_bytes()).expect("html");
        let section = &back.sections[0];
        let header = section.header.as_ref().expect("header");
        let footer = section.footer.as_ref().expect("footer");
        assert_eq!(
            header
                .blocks
                .iter()
                .filter_map(|b| match b {
                    Block::Paragraph(p) => Some(p.plain_text()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            ["Harbor"]
        );
        assert_eq!(
            footer
                .blocks
                .iter()
                .filter_map(|b| match b {
                    Block::Paragraph(p) => Some(p.plain_text()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            ["Page"]
        );
        assert_eq!(first_para(&back).plain_text(), "body");
    }

    #[test]
    fn read_roundtrips_ul() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Paragraph(list_item(
            "dock",
            NumberFormat::Bullet,
            0,
            None,
        )));
        section.body.push(Block::Paragraph(list_item(
            "bay 4",
            NumberFormat::Bullet,
            1,
            None,
        )));
        section.body.push(Block::Paragraph(list_item(
            "replay",
            NumberFormat::Bullet,
            0,
            None,
        )));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(
            html.contains("<ul><li>dock<ul><li>bay 4</li></ul></li><li>replay</li></ul>"),
            "{html}"
        );
        let back = read(html.as_bytes()).expect("html");
        let items: Vec<_> = numbered_paras(&back)
            .into_iter()
            .map(|p| {
                let n = p.numbering.as_ref().unwrap();
                (p.plain_text(), n.format, n.level, n.start)
            })
            .collect();
        assert_eq!(
            items,
            [
                ("dock".into(), Some(NumberFormat::Bullet), 0, None),
                ("bay 4".into(), Some(NumberFormat::Bullet), 1, None),
                ("replay".into(), Some(NumberFormat::Bullet), 0, None),
            ]
        );
    }

    #[test]
    fn read_roundtrips_ol() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Paragraph(list_item(
            "Hash the input",
            NumberFormat::Decimal,
            0,
            Some(1),
        )));
        section.body.push(Block::Paragraph(list_item(
            "Run Convert",
            NumberFormat::Decimal,
            0,
            Some(2),
        )));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("gap")));
        section.body.push(Block::Paragraph(list_item(
            "Seal the crate",
            NumberFormat::Decimal,
            0,
            Some(3),
        )));
        section.body.push(Block::Paragraph(list_item(
            "Ship the proof",
            NumberFormat::Decimal,
            0,
            Some(4),
        )));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(
            html.contains("<ol><li>Hash the input</li><li>Run Convert</li></ol>"),
            "{html}"
        );
        assert!(
            html.contains(r#"<ol start="3"><li>Seal the crate</li><li>Ship the proof</li></ol>"#),
            "{html}"
        );
        let back = read(html.as_bytes()).expect("html");
        let items: Vec<_> = numbered_paras(&back)
            .into_iter()
            .map(|p| {
                let n = p.numbering.as_ref().unwrap();
                (p.plain_text(), n.format, n.level, n.start)
            })
            .collect();
        assert_eq!(
            items,
            [
                (
                    "Hash the input".into(),
                    Some(NumberFormat::Decimal),
                    0,
                    Some(1)
                ),
                (
                    "Run Convert".into(),
                    Some(NumberFormat::Decimal),
                    0,
                    Some(2)
                ),
                (
                    "Seal the crate".into(),
                    Some(NumberFormat::Decimal),
                    0,
                    Some(3)
                ),
                (
                    "Ship the proof".into(),
                    Some(NumberFormat::Decimal),
                    0,
                    Some(4)
                ),
            ]
        );
    }

    #[test]
    fn read_roundtrips_task_list() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Paragraph(list_item(
            "Replay the convert",
            NumberFormat::Task,
            0,
            Some(1),
        )));
        section.body.push(Block::Paragraph(list_item(
            "Hope",
            NumberFormat::Task,
            0,
            None,
        )));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains(r#"<ul class="tasks">"#), "{html}");
        assert!(html.contains(r#"checked=""/>Replay the convert"#), "{html}");
        assert!(html.contains(r#"disabled=""/>Hope"#), "{html}");
        let back = read(html.as_bytes()).expect("html");
        let items: Vec<_> = numbered_paras(&back)
            .into_iter()
            .map(|p| {
                let n = p.numbering.as_ref().unwrap();
                (p.plain_text(), n.format, n.level, n.start)
            })
            .collect();
        assert_eq!(
            items,
            [
                (
                    "Replay the convert".into(),
                    Some(NumberFormat::Task),
                    0,
                    Some(1)
                ),
                ("Hope".into(), Some(NumberFormat::Task), 0, None),
            ]
        );
    }

    #[test]
    fn article_css_heading_size_line_height_pad_and_img_floor() {
        let html = to_html(&Document::new());
        assert!(
            html.contains("h1{font-size:1.75rem;line-height:1.2"),
            "h1 must stay letter CSS 1.75rem / leading 1.2: {html}"
        );
        assert!(
            html.contains("h2{font-size:1.15rem;line-height:1.2"),
            "h2 must stay letter CSS 1.15rem / leading 1.2: {html}"
        );
        assert!(
            html.contains("font-size:10pt;line-height:1.45"),
            "body type must stay 10pt / CSS 1.45: {html}"
        );
        assert!(
            html.contains(r#"article{max-width:48rem"#),
            "readable measure must stay 48rem: {html}"
        );
        assert!(
            !html.contains("h1{font-size:2em") && !html.contains("h1{font-size:18pt"),
            "UA heading size is not WeasyPrint letter type: {html}"
        );
        assert!(
            html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem"),
            "table cells must keep WeasyPrint padding: {html}"
        );
        assert!(
            html.contains("img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"),
            "24pt img floor and mark gap must not regress: {html}"
        );
        assert!(html.contains("p{margin:0 0 .7rem}"), "{html}");
    }

    #[test]
    fn html_read_headings_match_weasyprint_type() {
        let html = b"<article><h1>Northwind Freight</h1><h2>Delivery confirmation</h2><p>The shipment left Busan.</p></article>";
        let doc = read(html).expect("html");
        let h1 = match &doc.sections[0].body[0] {
            Block::Paragraph(p) => p,
            _ => panic!("h1"),
        };
        let h2 = match &doc.sections[0].body[1] {
            Block::Paragraph(p) => p,
            _ => panic!("h2"),
        };
        let body = match &doc.sections[0].body[2] {
            Block::Paragraph(p) => p,
            _ => panic!("body"),
        };
        assert_eq!(h1.outline_level, Some(1));
        assert_eq!(h1.runs[0].style.size, H1_SIZE_HU);
        assert_eq!(h1.runs[0].style.size, 2100);
        assert_ne!(h1.runs[0].style.size, 1800);
        assert!(h1.runs[0].style.bold);
        assert_eq!(h1.line_spacing, LineSpacing::Percent(HEADING_LEADING_PCT));
        assert_eq!(h2.outline_level, Some(2));
        assert_eq!(h2.runs[0].style.size, H2_SIZE_HU);
        assert_eq!(h2.runs[0].style.size, 1380);
        assert_ne!(h2.runs[0].style.size, 1400);
        assert!(h2.runs[0].style.bold);
        assert_eq!(h2.line_spacing, LineSpacing::Percent(HEADING_LEADING_PCT));
        assert_eq!(body.outline_level, None);
        assert_eq!(body.line_spacing, LineSpacing::Percent(BODY_LEADING_PCT));
        assert_eq!(body.line_spacing, LineSpacing::Percent(145));
        assert_ne!(body.line_spacing, LineSpacing::Percent(160));
        let out = to_html(&doc);
        assert!(
            out.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{out}"
        );
        assert!(
            out.contains("h2{font-size:1.15rem;line-height:1.2"),
            "{out}"
        );
        assert!(out.contains("font-size:10pt;line-height:1.45"), "{out}");
        assert!(out.contains("padding:.4rem .65rem"), "{out}");
        assert!(out.contains("img{min-width:24pt;min-height:24pt"), "{out}");
    }

    #[test]
    fn letter_md_html_heading_size_line_height_pad_and_img_floor() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let h1 = match &doc.sections[0].body[0] {
            Block::Paragraph(p) => p,
            _ => panic!("h1"),
        };
        let h2 = match &doc.sections[0].body[1] {
            Block::Paragraph(p) => p,
            _ => panic!("h2"),
        };
        assert_eq!(h1.runs[0].style.size, H1_SIZE_HU);
        assert_eq!(h1.line_spacing, LineSpacing::Percent(HEADING_LEADING_PCT));
        assert_eq!(h2.runs[0].style.size, H2_SIZE_HU);
        assert_eq!(h2.line_spacing, LineSpacing::Percent(HEADING_LEADING_PCT));
        let img = first_image(&doc);
        assert!(img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU);
        let img_p = first_image_paragraph(&doc);
        assert_eq!(img_p.space_after, IMAGE_AFTER_HU);
        let t = first_table(&doc);
        assert_eq!(t.rows[0].cells[0].padding.left, CELL_PAD_X_HU);
        assert_eq!(t.rows[0].cells[0].padding.top, CELL_PAD_Y_HU);
        let html = to_html(&doc);
        assert!(
            html.contains("<h1>") && html.contains("Northwind Freight"),
            "{html}"
        );
        assert!(
            html.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{html}"
        );
        assert!(
            html.contains("h2{font-size:1.15rem;line-height:1.2"),
            "{html}"
        );
        assert!(html.contains("font-size:10pt;line-height:1.45"), "{html}");
        assert!(html.contains(r#"article{max-width:48rem"#), "{html}");
        assert!(
            html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem"),
            "{html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "letter.md HTML th must keep WeasyPrint teal fill: {html}"
        );
        assert!(
            html.contains("img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"),
            "{html}"
        );
        assert!(html.contains("p{margin:0 0 .7rem}"), "{html}");
        assert!(
            html.contains("ul,ol{margin:0 0 1rem;padding-left:1.25rem}"),
            "letter.md HTML list indent must stay 1.25rem: {html}"
        );
        assert!(html.contains("li{margin:0 0 .35rem}"), "{html}");
        assert!(
            html.contains(
                "blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem;border-left:4px solid #134e4a;color:#134e4a}"
            ),
            "letter.md HTML quote bar must stay 4px #134e4a: {html}"
        );
        assert!(
            html.contains("pre{background:#ccfbf1;padding:.75rem 1rem;margin:.6rem 0 1rem"),
            "letter.md HTML pre must keep 0.6rem/1rem: {html}"
        );
    }

    #[test]
    fn article_css_list_quote_pre_and_landed_weasyprint() {
        let html = to_html(&Document::new());
        assert!(
            html.contains("ul,ol{margin:0 0 1rem;padding-left:1.25rem}"),
            "list indent must stay letter CSS 1.25rem, not UA 40px: {html}"
        );
        assert!(
            html.contains(NESTED_LIST_RULE)
                && html.contains("ul ul,ol ul,ul ol,ol ol{margin:.25rem 0}"),
            "nested lists must not be jammed or sparse: {html}"
        );
        assert!(
            html.contains("li{margin:0 0 .35rem}"),
            "list item gap must stay letter CSS 0.35rem: {html}"
        );
        assert!(
            html.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "task list indent must match ul 1.25rem: {html}"
        );
        assert!(
            html.contains("ul.tasks>li{list-style:none}"),
            "task items must not keep a UA disc beside the checkbox: {html}"
        );
        assert!(
            html.contains("ul.tasks input[type=checkbox]{appearance:none;-webkit-appearance:none;display:inline-block;width:.9em;height:.9em;margin-right:.4rem;vertical-align:middle;border:1.5pt solid #134e4a;border-radius:2px;background:#fff;accent-color:#134e4a}"),
            "task checkbox gap must stay letter CSS 0.4rem and paint as a WeasyPrint box: {html}"
        );
        assert!(
            html.contains("ul.tasks input[type=checkbox]:checked{background:#134e4a}"),
            "checked task boxes must fill teal: {html}"
        );
        assert!(
            html.contains(
                "blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem;border-left:4px solid #134e4a;color:#134e4a}"
            ),
            "blockquote must keep WeasyPrint 4px #134e4a left bar, not jammed (no margin) or sparse (UA 40px): {html}"
        );
        assert!(
            html.contains("pre{background:#ccfbf1;padding:.75rem 1rem;margin:.6rem 0 1rem;border-radius:4px;overflow:auto;font-family:ui-monospace,Consolas,monospace}"),
            "pre must keep letter CSS 0.6rem/1rem spacing and Consolas: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "inline code must keep letter.html ui-monospace/Consolas: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark highlight must stay visible: {html}"
        );
        assert!(
            !html.contains("ul,ol{margin:0;padding-left:40px")
                && !html.contains("blockquote{margin:1em 40px"),
            "UA list/quote spacing is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains("img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem"),
            "cell pad must not regress: {html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th fill must not regress: {html}"
        );
        assert!(
            html.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{html}"
        );
        assert!(
            html.contains("h2{font-size:1.15rem;line-height:1.2"),
            "{html}"
        );
        assert!(html.contains("font-size:10pt;line-height:1.45"), "{html}");
        assert!(html.contains("p{margin:0 0 .7rem}"), "{html}");
        assert!(html.contains(r#"article{max-width:48rem"#), "{html}");
    }

    #[test]
    fn html_read_lists_use_weasyprint_indent_and_spacing() {
        let html = b"<ul><li>dock is clear<ul><li>bay 4</li></ul></li><li>replay the convert</li></ul><p>Body after.</p>";
        let doc = read(html).expect("html");
        let items = numbered_paras(&doc);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].indent_left, LIST_INDENT_HU);
        assert_eq!(items[0].indent_left, 1500);
        assert_ne!(
            items[0].indent_left, 1440,
            "Hangul 1in list indent is not WeasyPrint 1.25rem"
        );
        assert_eq!(items[0].space_after, LIST_ITEM_AFTER_HU);
        assert_eq!(items[0].space_after, 420);
        assert_ne!(
            items[0].space_after, 80,
            "IR list space_after 80 is jammed vs letter CSS 0.35rem"
        );
        assert_eq!(items[1].indent_left, LIST_INDENT_HU * 2);
        assert_eq!(items[1].space_after, LIST_ITEM_AFTER_HU);
        assert_eq!(items[2].indent_left, LIST_INDENT_HU);
        assert_eq!(items[2].space_after, LIST_AFTER_HU);
        assert_eq!(items[2].space_after, 1200);
        assert_ne!(items[2].space_after, 80);
        let ol = read(b"<ol><li>Hash the input</li><li>Run Convert</li></ol>").expect("ol");
        let nums = numbered_paras(&ol);
        assert_eq!(nums[0].indent_left, LIST_INDENT_HU);
        assert_eq!(nums[0].space_after, LIST_ITEM_AFTER_HU);
        assert_eq!(nums[1].space_after, LIST_AFTER_HU);
        let out = to_html(&doc);
        assert!(
            out.contains("ul,ol{margin:0 0 1rem;padding-left:1.25rem}"),
            "{out}"
        );
        assert!(out.contains("li{margin:0 0 .35rem}"), "{out}");
        assert!(out.contains("img{min-width:24pt;min-height:24pt"), "{out}");
        assert!(out.contains("padding:.4rem .65rem"), "{out}");
        assert!(
            out.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{out}"
        );
    }

    #[test]
    fn html_read_quotes_and_pre_use_weasyprint_spacing() {
        let quote_html = b"<blockquote>Replay is evidence. Same fonts, same bytes.</blockquote>";
        let doc = read(quote_html).expect("html");
        let p = first_para(&doc);
        assert!(p.quote);
        assert_eq!(p.space_before, QUOTE_BEFORE_HU);
        assert_eq!(p.space_before, 600);
        assert_ne!(
            p.space_before, 80,
            "IR quote space_before 80 is jammed vs letter CSS 0.5rem"
        );
        assert_eq!(p.space_after, QUOTE_AFTER_HU);
        assert_eq!(p.space_after, 1200);
        assert_ne!(
            p.space_after, 200,
            "IR quote space_after 200 is jammed vs letter CSS 1rem"
        );
        assert_eq!(p.line_spacing, LineSpacing::Percent(BODY_LEADING_PCT));
        assert!(
            p.runs.iter().any(|r| {
                r.style.color == QUOTE_FG
                    && matches!(&r.content, RunContent::Text(t) if t.contains("Replay"))
            }),
            "html read quote text must stay letter CSS #134e4a (bar + color)"
        );
        assert_eq!(QUOTE_BAR_W_HU, 300, "4px bar at 96dpi is 300 HU");
        let pre_html = b"<pre><code>docagent prove examples/letter.md</code></pre>";
        let pre = read(pre_html).expect("html");
        let c = first_para(&pre);
        assert!(c.code_block);
        assert!(
            c.runs.iter().any(|r| {
                r.style.font.as_deref() == Some(CODE_FONT)
                    && r.style.latin_font.as_deref() == Some(CODE_FONT)
            }),
            "html <pre> must keep letter CSS Consolas, not generic serif: {:?}",
            c.runs
                .iter()
                .map(|r| r.style.font.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(c.space_before, CODE_BEFORE_HU);
        assert_eq!(c.space_before, 720);
        assert_ne!(
            c.space_before, 80,
            "IR pre space_before 80 is jammed vs letter CSS 0.6rem"
        );
        assert_eq!(c.space_after, CODE_AFTER_HU);
        assert_eq!(c.space_after, 1200);
        assert_ne!(
            c.space_after, 200,
            "IR pre space_after 200 is jammed vs letter CSS 1rem"
        );
        let out = to_html(&doc);
        assert!(
            out.contains(
                "blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem;border-left:4px solid #134e4a;color:#134e4a}"
            ),
            "round-trip HTML must keep the 4px #134e4a quote bar: {out}"
        );
        assert!(
            out.contains("pre{background:#ccfbf1;padding:.75rem 1rem;margin:.6rem 0 1rem"),
            "{out}"
        );
        assert!(out.contains("img{min-width:24pt;min-height:24pt"), "{out}");
        assert!(out.contains("padding:.4rem .65rem"), "{out}");
        assert!(
            out.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{out}"
        );
        assert!(out.contains("font-size:10pt;line-height:1.45"), "{out}");
    }

    #[test]
    fn letter_md_html_list_quote_pre_spacing() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let quote = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) if p.quote => Some(p),
                _ => None,
            })
            .expect("quote");
        assert_eq!(quote.space_before, QUOTE_BEFORE_HU);
        assert_eq!(quote.space_after, QUOTE_AFTER_HU);
        assert!(
            quote.runs.iter().any(|r| r.style.color == QUOTE_FG),
            "letter.md quote text must stay letter CSS #134e4a"
        );
        let code = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) if p.code_block => Some(p),
                _ => None,
            })
            .expect("pre");
        assert_eq!(code.space_before, CODE_BEFORE_HU);
        assert_eq!(code.space_after, CODE_AFTER_HU);
        let items = numbered_paras(&doc);
        assert!(!items.is_empty());
        assert_eq!(items[0].indent_left, LIST_INDENT_HU);
        assert_eq!(items[0].space_after, LIST_ITEM_AFTER_HU);
        assert_eq!(items.last().unwrap().space_after, LIST_AFTER_HU);
        let img = first_image(&doc);
        assert!(img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU);
        let t = first_table(&doc);
        assert_eq!(t.rows[0].cells[0].padding.left, CELL_PAD_X_HU);
        let html = to_html(&doc);
        assert!(html.contains("<blockquote>"), "{html}");
        assert!(html.contains("<pre><code>"), "{html}");
        assert!(
            html.contains("<ul class=\"tasks\">") || html.contains("<ol>"),
            "{html}"
        );
        assert!(
            html.contains("ul,ol{margin:0 0 1rem;padding-left:1.25rem}"),
            "{html}"
        );
        assert!(html.contains("li{margin:0 0 .35rem}"), "{html}");
        assert!(
            html.contains(
                "blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem;border-left:4px solid #134e4a;color:#134e4a}"
            ),
            "letter.md HTML quote bar must stay 4px #134e4a: {html}"
        );
        assert!(
            html.contains("pre{background:#ccfbf1;padding:.75rem 1rem;margin:.6rem 0 1rem"),
            "{html}"
        );
        assert!(
            html.contains("img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"),
            "{html}"
        );
        assert!(
            html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem"),
            "{html}"
        );
        assert!(
            html.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{html}"
        );
        assert!(
            html.contains("h2{font-size:1.15rem;line-height:1.2"),
            "{html}"
        );
        assert!(html.contains("font-size:10pt;line-height:1.45"), "{html}");
        assert!(html.contains("p{margin:0 0 .7rem}"), "{html}");
        assert!(
            html.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"),
            "letter.md HTML hr must stay letter.css 3px / 1.1rem: {html}"
        );
        assert!(
            html.contains(LINK_RULE) && html.contains("a{color:#0d9488;text-decoration:underline}"),
            "letter.md HTML links must stay teal and underlined: {html}"
        );
        assert!(
            html.contains(LINK_HOVER_RULE) && html.contains("a:hover{color:#134e4a}"),
            "letter.md HTML link hover must rest to #134e4a: {html}"
        );
    }

    #[test]
    fn article_css_hr_link_and_landed_weasyprint() {
        let html = to_html(&Document::new());
        assert!(
            html.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"),
            "hr thickness/spacing must stay letter.html 3px / 1.1rem: {html}"
        );
        assert!(
            !html.contains("hr{border:1px")
                && !html.contains("hr{margin:.5em")
                && !html.contains("hr{margin:0"),
            "UA 1px inset / 0.5em hr is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(LINK_RULE) && html.contains("a{color:#0d9488;text-decoration:underline}"),
            "links must stay #0d9488 and underlined like WeasyPrint: {html}"
        );
        assert!(
            html.contains(LINK_HOVER_RULE) && html.contains("a:hover{color:#134e4a}"),
            "link hover must rest to letter teal #134e4a, not UA red/blue: {html}"
        );
        assert!(
            !html.contains("a{color:blue")
                && !html.contains("a{text-decoration:none")
                && !html.contains("a:hover{color:blue")
                && !html.contains("a:hover{color:red")
                && !html.contains("a:hover{color:#000"),
            "UA blue / no-underline / red hover hides letter links: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem"),
            "cell pad must not regress: {html}"
        );
        assert!(
            html.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{html}"
        );
        assert!(
            html.contains("h2{font-size:1.15rem;line-height:1.2"),
            "{html}"
        );
        assert!(html.contains("font-size:10pt;line-height:1.45"), "{html}");
        assert!(
            html.contains("ul,ol{margin:0 0 1rem;padding-left:1.25rem}"),
            "list indent must not regress: {html}"
        );
        assert!(html.contains("li{margin:0 0 .35rem}"), "{html}");
        assert!(
            html.contains("blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem"),
            "quote must not regress: {html}"
        );
        assert!(html.contains("p{margin:0 0 .7rem}"), "{html}");
        assert!(html.contains(r#"article{max-width:48rem"#), "{html}");
    }

    #[test]
    fn letter_md_html_hr_and_link_styles() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("---"),
            "letter.md must keep the thematic break"
        );
        assert!(
            text.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "letter.md must keep the DocAgent hyperlink"
        );
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        assert!(
            doc.sections[0]
                .body
                .iter()
                .any(|b| matches!(b, Block::Break(BreakKind::Thematic))),
            "letter.md IR must keep the thematic break"
        );
        let link = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) => p.runs.iter().find_map(|r| match &r.content {
                    RunContent::Inline(InlineObject::Hyperlink { target, display }) => {
                        Some((target.as_str(), display.as_str(), r.style.underline))
                    }
                    _ => None,
                }),
                _ => None,
            })
            .expect("letter hyperlink");
        assert_eq!(link.0, "https://github.com/kevin9327/docagent");
        assert_eq!(link.1, "DocAgent");
        assert_eq!(link.2, docagent_model::Underline::Single);
        let img = first_image(&doc);
        assert!(img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU);
        let t = first_table(&doc);
        assert_eq!(t.rows[0].cells[0].padding.left, CELL_PAD_X_HU);
        let html = to_html(&doc);
        assert!(html.contains("<hr/>"), "{html}");
        assert!(
            html.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"),
            "letter.md HTML hr must match letter.html: {html}"
        );
        assert!(
            html.contains(r#"<a href="https://github.com/kevin9327/docagent">DocAgent</a>"#),
            "{html}"
        );
        assert!(
            html.contains(LINK_RULE) && html.contains("a{color:#0d9488;text-decoration:underline}"),
            "letter.md HTML links must stay visible like WeasyPrint: {html}"
        );
        assert!(
            html.contains(LINK_HOVER_RULE) && html.contains("a:hover{color:#134e4a}"),
            "letter.md HTML link hover must rest to #134e4a: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem"),
            "{html}"
        );
        assert!(
            html.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{html}"
        );
        assert!(
            html.contains("h2{font-size:1.15rem;line-height:1.2"),
            "{html}"
        );
        assert!(
            html.contains("ul,ol{margin:0 0 1rem;padding-left:1.25rem}"),
            "{html}"
        );
        assert!(
            html.contains("blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem"),
            "{html}"
        );
    }

    #[test]
    fn html_read_hr_and_link_roundtrip() {
        let html = b"<article><p>before</p><hr/><p>after <a href=\"https://github.com/kevin9327/docagent\">DocAgent</a></p></article>";
        let doc = read(html).expect("html");
        assert!(
            doc.sections[0]
                .body
                .iter()
                .any(|b| matches!(b, Block::Break(BreakKind::Thematic))),
            "html read must keep <hr> as a thematic break"
        );
        let p = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) if p.plain_text().contains("DocAgent") => Some(p),
                _ => None,
            })
            .expect("link para");
        assert!(p.runs.iter().any(|r| matches!(
            &r.content,
            RunContent::Inline(InlineObject::Hyperlink { target, display })
                if target == "https://github.com/kevin9327/docagent" && display == "DocAgent"
        )));
        assert!(
            p.runs.iter().any(|r| {
                r.style.underline == docagent_model::Underline::Single
                    && matches!(
                        &r.content,
                        RunContent::Inline(InlineObject::Hyperlink { .. })
                    )
            }),
            "html <a> must round-trip as an underlined hyperlink"
        );
        let out = to_html(&doc);
        assert!(out.contains("<hr/>"), "{out}");
        assert!(
            out.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"),
            "{out}"
        );
        assert!(
            out.contains("a{color:#0d9488;text-decoration:underline}"),
            "{out}"
        );
        assert!(out.contains("img{min-width:24pt;min-height:24pt"), "{out}");
        assert!(out.contains("padding:.4rem .65rem"), "{out}");
        assert!(
            out.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{out}"
        );
        assert!(
            out.contains("ul,ol{margin:0 0 1rem;padding-left:1.25rem}"),
            "{out}"
        );
        assert!(
            out.contains("blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem"),
            "{out}"
        );
    }

    #[test]
    fn article_css_inline_code_mark_and_landed_weasyprint() {
        let html = to_html(&Document::new());
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "inline code must keep letter.html chip (background + padding), not a bare UA span: {html}"
        );
        assert!(
            html.contains("pre code{background:transparent;padding:0;color:#134e4a;font-family:ui-monospace,Consolas,monospace}"),
            "pre code must not inherit the inline-code chip and must keep Consolas: {html}"
        );
        assert!(
            !html.contains("code{background:none")
                && !html.contains("code{padding:0;color:inherit"),
            "reset code styles hide WeasyPrint letter chips: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark must stay letter.html amber so ==highlight== is visible: {html}"
        );
        assert!(
            !html.contains("mark{background:transparent")
                && !html.contains("mark{background:none")
                && !html.contains("mark{background:yellow"),
            "UA/reset mark hides letter highlights: {html}"
        );
        assert!(
            html.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"),
            "hr must not regress: {html}"
        );
        assert!(
            html.contains("a{color:#0d9488;text-decoration:underline}"),
            "link must not regress: {html}"
        );
        assert!(
            html.contains("ul,ol{margin:0 0 1rem;padding-left:1.25rem}"),
            "list indent must not regress: {html}"
        );
        assert!(html.contains("li{margin:0 0 .35rem}"), "{html}");
        assert!(
            html.contains("blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem"),
            "quote must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem"),
            "cell pad must not regress: {html}"
        );
        assert!(
            html.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{html}"
        );
        assert!(
            html.contains("h2{font-size:1.15rem;line-height:1.2"),
            "{html}"
        );
        assert!(html.contains("font-size:10pt;line-height:1.45"), "{html}");
        assert!(html.contains("p{margin:0 0 .7rem}"), "{html}");
        assert!(html.contains(r#"article{max-width:48rem"#), "{html}");
    }

    #[test]
    fn letter_md_html_inline_code_and_mark_styles() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("`prove`"),
            "letter.md must keep the inline code span WeasyPrint paints as a chip"
        );
        assert!(
            text.contains("==three hashes=="),
            "letter.md must keep the ==highlight== WeasyPrint paints as <mark>"
        );
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let body = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p)
                    if p.outline_level.is_none()
                        && p.numbering.is_none()
                        && p.plain_text().contains("shipment") =>
                {
                    Some(p)
                }
                _ => None,
            })
            .expect("body");
        assert!(
            body.runs.iter().any(|r| {
                r.style.code && matches!(&r.content, RunContent::Text(t) if t == "prove")
            }),
            "letter.md IR must keep `prove` as inline code"
        );
        assert!(
            body.runs.iter().any(|r| {
                r.style.highlight == Some(docagent_model::Color::MARK)
                    && matches!(&r.content, RunContent::Text(t) if t == "three hashes")
            }),
            "letter.md IR must keep ==three hashes== as Color::MARK"
        );
        let img = first_image(&doc);
        assert!(img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU);
        let t = first_table(&doc);
        assert_eq!(t.rows[0].cells[0].padding.left, CELL_PAD_X_HU);
        let html = to_html(&doc);
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(html.contains("<mark>three hashes</mark>"), "{html}");
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "letter.md HTML inline code must match letter.html: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "letter.md HTML mark must stay visible like WeasyPrint: {html}"
        );
        assert!(
            html.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"),
            "hr must not regress: {html}"
        );
        assert!(
            html.contains("a{color:#0d9488;text-decoration:underline}"),
            "link must not regress: {html}"
        );
        assert!(
            html.contains("ul,ol{margin:0 0 1rem;padding-left:1.25rem}"),
            "{html}"
        );
        assert!(
            html.contains("blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem"),
            "{html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem"),
            "{html}"
        );
        assert!(
            html.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{html}"
        );
        assert!(
            html.contains("h2{font-size:1.15rem;line-height:1.2"),
            "{html}"
        );
    }

    #[test]
    fn html_read_code_and_mark_roundtrip() {
        let html = b"<article><p>Run <code>prove</code> twice with <mark>three hashes</mark>.</p></article>";
        let doc = read(html).expect("html");
        let p = first_para(&doc);
        assert!(
            p.runs.iter().any(|r| {
                r.style.code
                    && r.style.font.as_deref() == Some(CODE_FONT)
                    && r.style.latin_font.as_deref() == Some(CODE_FONT)
                    && matches!(&r.content, RunContent::Text(t) if t == "prove")
            }),
            "html read must keep <code> as Consolas inline code, not article serif"
        );
        assert!(
            p.runs.iter().any(|r| {
                r.style.highlight == Some(docagent_model::Color::MARK)
                    && matches!(&r.content, RunContent::Text(t) if t == "three hashes")
            }),
            "html read must keep <mark> as Color::MARK"
        );
        assert!(!p.code_block);
        let out = to_html(&doc);
        assert!(out.contains("<code>prove</code>"), "{out}");
        assert!(out.contains("<mark>three hashes</mark>"), "{out}");
        assert!(
            out.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "{out}"
        );
        assert!(
            out.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "{out}"
        );
        assert!(
            out.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"),
            "{out}"
        );
        assert!(
            out.contains("a{color:#0d9488;text-decoration:underline}"),
            "{out}"
        );
        assert!(out.contains("img{min-width:24pt;min-height:24pt"), "{out}");
        assert!(out.contains("padding:.4rem .65rem"), "{out}");
        assert!(
            out.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{out}"
        );
        assert!(
            out.contains("ul,ol{margin:0 0 1rem;padding-left:1.25rem}"),
            "{out}"
        );
        assert!(
            out.contains("blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem"),
            "{out}"
        );
    }

    #[test]
    fn article_css_task_checkboxes_and_landed_weasyprint() {
        let html = to_html(&Document::new());
        assert_eq!(
            TASKS_RULE,
            "ul.tasks{list-style:none;padding-left:1.25rem}ul.tasks>li{list-style:none}ul.tasks ul{list-style:disc}ul.tasks input[type=checkbox]{appearance:none;-webkit-appearance:none;display:inline-block;width:.9em;height:.9em;margin-right:.4rem;vertical-align:middle;border:1.5pt solid #134e4a;border-radius:2px;background:#fff;accent-color:#134e4a}ul.tasks input[type=checkbox]:checked{background:#134e4a}"
        );
        assert!(
            html.contains(TASKS_RULE),
            "article CSS must pin ul.tasks so UA discs / 40px gutters do not punch the 48rem column: {html}"
        );
        assert!(
            html.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "task lists must drop the disc so - [ ] is a checkbox, not a missing marker: {html}"
        );
        assert!(
            html.contains("ul.tasks>li{list-style:none}"),
            "GitHub/WeasyPrint task items must not keep a UA disc: {html}"
        );
        assert!(
            html.contains("ul.tasks ul{list-style:disc}"),
            "nested bullets inside a task list must keep discs: {html}"
        );
        assert!(
            html.contains("ul.tasks input[type=checkbox]{appearance:none;-webkit-appearance:none;display:inline-block;width:.9em;height:.9em;margin-right:.4rem;vertical-align:middle;border:1.5pt solid #134e4a;border-radius:2px;background:#fff;accent-color:#134e4a}"),
            "WeasyPrint native checkboxes are missing; CSS must draw the box: {html}"
        );
        assert!(
            html.contains("ul.tasks input[type=checkbox]:checked{background:#134e4a}"),
            "checked - [x] must fill teal like letter.html: {html}"
        );
        assert!(
            !html.contains("ul.tasks input{display:none")
                && !html.contains("input[type=checkbox]{display:none")
                && !html.contains("input[type=checkbox]{visibility:hidden")
                && !html.contains("input[type=checkbox]{opacity:0"),
            "hiding the checkbox leaves - [ ] looking empty: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "inline code chip must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark highlight must not regress: {html}"
        );
        assert!(
            html.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"),
            "hr must not regress: {html}"
        );
        assert!(
            html.contains("a{color:#0d9488;text-decoration:underline}"),
            "link must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem"),
            "cell pad must not regress: {html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th fill must not regress: {html}"
        );
        assert!(
            html.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{html}"
        );
        assert!(
            html.contains("h2{font-size:1.15rem;line-height:1.2"),
            "{html}"
        );
        assert!(html.contains("font-size:10pt;line-height:1.45"), "{html}");
        assert!(
            html.contains("ul,ol{margin:0 0 1rem;padding-left:1.25rem}"),
            "list indent must not regress: {html}"
        );
        assert!(html.contains("li{margin:0 0 .35rem}"), "{html}");
        assert!(
            html.contains("blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem"),
            "quote must not regress: {html}"
        );
        assert!(html.contains("p{margin:0 0 .7rem}"), "{html}");
        assert!(html.contains(r#"article{max-width:48rem"#), "{html}");
    }

    #[test]
    fn letter_md_html_task_checkboxes() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("- [x] Receiving dock is clear"),
            "letter.md must keep the checked task item"
        );
        assert!(
            text.contains("- [ ] Replay the convert instead of hoping"),
            "letter.md must keep the unchecked task item"
        );
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let tasks: Vec<_> = numbered_paras(&doc)
            .into_iter()
            .filter(|p| {
                p.numbering
                    .as_ref()
                    .is_some_and(|n| n.format == Some(NumberFormat::Task))
            })
            .collect();
        assert!(
            tasks.len() >= 3,
            "letter.md IR must keep - [x] / - [ ] as NumberFormat::Task"
        );
        assert!(
            tasks.iter().any(|p| {
                p.plain_text().contains("Receiving dock")
                    && p.numbering.as_ref().is_some_and(|n| n.start == Some(1))
            }),
            "checked - [x] must round-trip as start=1"
        );
        assert!(
            tasks.iter().any(|p| {
                p.plain_text().contains("Replay the convert")
                    && p.numbering.as_ref().is_some_and(|n| n.start.is_none())
            }),
            "unchecked - [ ] must round-trip as start=None"
        );
        let html = to_html(&doc);
        assert!(html.contains(r#"<ul class="tasks">"#), "{html}");
        assert!(
            html.contains(
                r#"<input type="checkbox" disabled="" checked=""/>Receiving dock is clear"#
            ),
            "letter.md - [x] must emit a checked checkbox, not a disc: {html}"
        );
        assert!(
            html.contains(
                r#"<input type="checkbox" disabled=""/>Replay the convert instead of hoping"#
            ),
            "letter.md - [ ] must emit an unchecked checkbox: {html}"
        );
        assert!(
            !html.contains("- [x]") && !html.contains("- [ ]"),
            "raw GFM task markers must not leak into HTML: {html}"
        );
        assert!(
            html.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "letter.md HTML task lists must drop the disc: {html}"
        );
        assert!(html.contains("ul.tasks>li{list-style:none}"), "{html}");
        assert!(
            html.contains("ul.tasks ul{list-style:disc}"),
            "nested Bay 4 under a task must keep a disc: {html}"
        );
        assert!(
            html.contains("ul.tasks input[type=checkbox]{appearance:none;-webkit-appearance:none;display:inline-block;width:.9em;height:.9em;margin-right:.4rem;vertical-align:middle;border:1.5pt solid #134e4a;border-radius:2px;background:#fff;accent-color:#134e4a}"),
            "letter.md HTML checkboxes must paint as WeasyPrint CSS boxes: {html}"
        );
        assert!(
            html.contains("ul.tasks input[type=checkbox]:checked{background:#134e4a}"),
            "letter.md HTML checked tasks must fill teal: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "inline code chip must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark highlight must not regress: {html}"
        );
        assert!(
            html.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"),
            "hr must not regress: {html}"
        );
        assert!(
            html.contains("a{color:#0d9488;text-decoration:underline}"),
            "link must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem"),
            "{html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "letter.md HTML th must keep WeasyPrint teal fill: {html}"
        );
        assert!(
            html.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{html}"
        );
        assert!(
            html.contains("h2{font-size:1.15rem;line-height:1.2"),
            "{html}"
        );
        let img = first_image(&doc);
        assert!(img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU);
        let t = first_table(&doc);
        assert_eq!(t.rows[0].cells[0].padding.left, CELL_PAD_X_HU);
        assert_eq!(t.rows[0].cells[0].shading, Some(TH_BG));
    }

    #[test]
    fn html_read_task_checkboxes_roundtrip() {
        let html = b"<ul class=\"tasks\"><li><input type=\"checkbox\" disabled=\"\" checked=\"\"/>Replay the convert</li><li><input type=\"checkbox\" disabled=\"\"/>Hope</li></ul>";
        let doc = read(html).expect("html");
        let items: Vec<_> = numbered_paras(&doc)
            .into_iter()
            .map(|p| {
                let n = p.numbering.as_ref().unwrap();
                (p.plain_text(), n.format, n.start)
            })
            .collect();
        assert_eq!(
            items,
            [
                (
                    "Replay the convert".into(),
                    Some(NumberFormat::Task),
                    Some(1)
                ),
                ("Hope".into(), Some(NumberFormat::Task), None),
            ]
        );
        assert_eq!(numbered_paras(&doc)[0].indent_left, LIST_INDENT_HU);
        assert_eq!(numbered_paras(&doc)[0].space_after, LIST_ITEM_AFTER_HU);
        assert_eq!(numbered_paras(&doc)[1].space_after, LIST_AFTER_HU);
        let out = to_html(&doc);
        assert!(out.contains(r#"<ul class="tasks">"#), "{out}");
        assert!(
            out.contains(r#"<input type="checkbox" disabled="" checked=""/>Replay the convert"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<input type="checkbox" disabled=""/>Hope"#),
            "{out}"
        );
        assert!(
            out.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "{out}"
        );
        assert!(
            out.contains("ul.tasks input[type=checkbox]{appearance:none;-webkit-appearance:none;display:inline-block;width:.9em;height:.9em;margin-right:.4rem"),
            "{out}"
        );
        assert!(
            out.contains("ul.tasks input[type=checkbox]:checked{background:#134e4a}"),
            "{out}"
        );
        assert!(
            out.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "{out}"
        );
        assert!(
            out.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "{out}"
        );
        assert!(
            out.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"),
            "{out}"
        );
        assert!(
            out.contains("a{color:#0d9488;text-decoration:underline}"),
            "{out}"
        );
        assert!(out.contains("img{min-width:24pt;min-height:24pt"), "{out}");
        assert!(out.contains("padding:.4rem .65rem"), "{out}");
        assert!(
            out.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{out}"
        );
        assert!(
            out.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "{out}"
        );
    }

    #[test]
    fn article_css_table_header_and_landed_weasyprint() {
        let html = to_html(&Document::new());
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th must keep letter.html teal fill, teal text, 700 weight, and 2px #0d9488 header rule: {html}"
        );
        assert!(
            html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem;text-align:left;font-size:.95rem}"),
            "cell pad/border must stay letter CSS, not a UA 1px sparse grid: {html}"
        );
        assert!(
            !html.contains("th{background:transparent")
                && !html.contains("th{background:none")
                && !html.contains("th{background:#fff")
                && !html.contains("th{background:white")
                && !html.contains("th{font-weight:400")
                && !html.contains("th{border:none")
                && !html.contains("th{border-bottom:1px")
                && !html.contains("th{border-bottom:none"),
            "UA/reset th hides the WeasyPrint letter header: {html}"
        );
        assert!(
            html.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "task lists must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks input[type=checkbox]{appearance:none;-webkit-appearance:none;display:inline-block;width:.9em;height:.9em;margin-right:.4rem;vertical-align:middle;border:1.5pt solid #134e4a;border-radius:2px;background:#fff;accent-color:#134e4a}"),
            "task checkbox CSS boxes must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks input[type=checkbox]:checked{background:#134e4a}"),
            "checked tasks must not regress: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "inline code chip must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark highlight must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{html}"
        );
        assert!(
            html.contains("h2{font-size:1.15rem;line-height:1.2"),
            "{html}"
        );
        assert!(html.contains("font-size:10pt;line-height:1.45"), "{html}");
        assert!(html.contains("p{margin:0 0 .7rem}"), "{html}");
        assert!(html.contains(r#"article{max-width:48rem"#), "{html}");
        assert!(
            html.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"),
            "hr must not regress: {html}"
        );
        assert!(
            html.contains("a{color:#0d9488;text-decoration:underline}"),
            "link must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_html_table_header_styles() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("| Hop |"),
            "letter.md must keep the hop table"
        );
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let t = first_table(&doc);
        assert!(t.rows[0].header, "letter hop row must be a header");
        assert_eq!(t.header_row_count, 1);
        for cell in &t.rows[0].cells {
            assert_eq!(
                cell.shading,
                Some(TH_BG),
                "letter.md header cells must keep #ccfbf1 fill, not a sparse grid: {:?}",
                cell.shading
            );
            assert_eq!(cell.padding.left, CELL_PAD_X_HU);
            assert_eq!(cell.padding.top, CELL_PAD_Y_HU);
            assert!(
                cell.blocks.iter().any(|b| match b {
                    Block::Paragraph(p) => {
                        p.runs
                            .iter()
                            .any(|r| r.style.bold && r.style.color == TH_FG)
                    }
                    _ => false,
                }),
                "letter.md th text must stay bold #134e4a"
            );
        }
        for row in t.rows.iter().skip(1) {
            for cell in &row.cells {
                assert_ne!(cell.shading, Some(TH_BG));
            }
        }
        let html = to_html(&doc);
        assert!(html.contains("<th><strong>Hop</strong></th>"), "{html}");
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "letter.md HTML th must look like WeasyPrint, not a sparse grid: {html}"
        );
        assert!(
            html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem"),
            "{html}"
        );
        assert!(
            !html.contains("th{background:transparent")
                && !html.contains("th{background:none")
                && !html.contains("th{background:#fff"),
            "letter.md HTML th must not drop to a white sparse grid: {html}"
        );
        assert!(
            html.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "tasks must not regress: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "code chip must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        let img = first_image(&doc);
        assert!(img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU);
    }

    #[test]
    fn html_read_table_header_fill() {
        let html = b"<table><tr><th>Hop</th><th>File</th><th>Proof</th></tr><tr><td>Input</td><td>letter.md</td><td>input SHA-256</td></tr></table>";
        let doc = read(html).expect("html");
        let t = first_table(&doc);
        assert!(t.rows[0].header);
        assert!(!t.rows[1].header);
        assert_eq!(t.header_row_count, 1);
        assert_eq!(t.rows[0].cells.len(), 3);
        for cell in &t.rows[0].cells {
            assert_eq!(cell.shading, Some(TH_BG));
            assert_eq!(cell.padding.left, CELL_PAD_X_HU);
            assert_eq!(cell.padding.top, CELL_PAD_Y_HU);
            assert!(cell.blocks.iter().any(|b| {
                match b {
                    Block::Paragraph(p) => p
                        .runs
                        .iter()
                        .any(|r| r.style.bold && r.style.color == TH_FG),
                    _ => false,
                }
            }));
        }
        assert_eq!(t.rows[1].cells[0].shading, None);
        let out = to_html(&doc);
        assert!(out.contains("<th>"), "{out}");
        assert!(
            out.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "{out}"
        );
        assert!(
            out.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem"),
            "{out}"
        );
        assert!(out.contains("img{min-width:24pt;min-height:24pt"), "{out}");
        assert!(
            out.contains("code{background:#ccfbf1;padding:.1em .35em"),
            "{out}"
        );
        assert!(
            out.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "{out}"
        );
        assert!(
            out.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "{out}"
        );
    }

    #[test]
    fn article_css_strike_underline_and_landed_weasyprint() {
        let html = to_html(&Document::new());
        assert!(
            html.contains("s,del{text-decoration:line-through;color:#134e4a}"),
            "s/del must keep WeasyPrint teal line-through, not a missing UA strike: {html}"
        );
        assert!(
            html.contains("u{text-decoration:underline}"),
            "u must keep WeasyPrint underline, not a missing UA decoration: {html}"
        );
        assert!(
            !html.contains("s{text-decoration:none")
                && !html.contains("del{text-decoration:none")
                && !html.contains("s,del{text-decoration:none")
                && !html.contains("u{text-decoration:none")
                && !html.contains("s{text-decoration:line-through;color:#000")
                && !html.contains("s{text-decoration:line-through;color:#111"),
            "reset/black s/del/u hides letter strike and underline: {html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th fill must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "task lists must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks input[type=checkbox]{appearance:none;-webkit-appearance:none;display:inline-block;width:.9em;height:.9em;margin-right:.4rem;vertical-align:middle;border:1.5pt solid #134e4a;border-radius:2px;background:#fff;accent-color:#134e4a}"),
            "task checkbox CSS boxes must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks input[type=checkbox]:checked{background:#134e4a}"),
            "checked tasks must not regress: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "inline code chip must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark highlight must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem"),
            "cell pad must not regress: {html}"
        );
        assert!(
            html.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{html}"
        );
        assert!(
            html.contains("h2{font-size:1.15rem;line-height:1.2"),
            "{html}"
        );
        assert!(html.contains("font-size:10pt;line-height:1.45"), "{html}");
        assert!(html.contains("p{margin:0 0 .7rem}"), "{html}");
        assert!(html.contains(r#"article{max-width:48rem"#), "{html}");
        assert!(
            html.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"),
            "hr must not regress: {html}"
        );
        assert!(
            html.contains("a{color:#0d9488;text-decoration:underline}"),
            "link must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_html_strike_and_underline_styles() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("~~guess~~"),
            "letter.md must keep the strikethrough WeasyPrint paints as <s>"
        );
        assert!(
            text.contains("__09:00 local__"),
            "letter.md must keep the underline WeasyPrint paints as <u>"
        );
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let body = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p)
                    if p.outline_level.is_none()
                        && p.numbering.is_none()
                        && p.plain_text().contains("guess") =>
                {
                    Some(p)
                }
                _ => None,
            })
            .expect("strike para");
        assert!(
            body.runs.iter().any(|r| {
                r.style.strike && matches!(&r.content, RunContent::Text(t) if t == "guess")
            }),
            "letter.md IR must keep ~~guess~~ as strike"
        );
        let confirm = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) if p.plain_text().contains("09:00 local") => Some(p),
                _ => None,
            })
            .expect("underline para");
        assert!(
            confirm.runs.iter().any(|r| {
                r.style.underline == docagent_model::Underline::Single
                    && matches!(&r.content, RunContent::Text(t) if t == "09:00 local")
            }),
            "letter.md IR must keep __09:00 local__ as underline"
        );
        let img = first_image(&doc);
        assert!(img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU);
        let t = first_table(&doc);
        assert_eq!(t.rows[0].cells[0].padding.left, CELL_PAD_X_HU);
        assert_eq!(t.rows[0].cells[0].shading, Some(TH_BG));
        assert!(
            numbered_paras(&doc).iter().any(|p| p
                .numbering
                .as_ref()
                .is_some_and(|n| n.format == Some(NumberFormat::Task))),
            "tasks must not regress"
        );
        let html = to_html(&doc);
        assert!(html.contains("<s>guess</s>"), "{html}");
        assert!(html.contains("<u>09:00 local</u>"), "{html}");
        assert!(
            html.contains("s,del{text-decoration:line-through;color:#134e4a}"),
            "letter.md HTML strike must look like WeasyPrint, not missing: {html}"
        );
        assert!(
            html.contains("u{text-decoration:underline}"),
            "letter.md HTML underline must look like WeasyPrint, not missing: {html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th fill must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "tasks must not regress: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "code chip must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(html.contains("<mark>three hashes</mark>"), "{html}");
        assert!(html.contains(r#"<ul class="tasks">"#), "{html}");
    }

    #[test]
    fn html_read_strike_and_underline_roundtrip() {
        let html = b"<article><p>Agents <s>guess</s>. Please confirm before <u>09:00 local</u>. Do not <del>hope</del>.</p></article>";
        let doc = read(html).expect("html");
        let p = first_para(&doc);
        assert!(
            p.runs.iter().any(|r| {
                r.style.strike && matches!(&r.content, RunContent::Text(t) if t == "guess")
            }),
            "html read must keep <s> as a strike run"
        );
        assert!(
            p.runs.iter().any(|r| {
                r.style.strike && matches!(&r.content, RunContent::Text(t) if t == "hope")
            }),
            "html read must keep <del> as a strike run"
        );
        assert!(
            p.runs.iter().any(|r| {
                r.style.underline == docagent_model::Underline::Single
                    && matches!(&r.content, RunContent::Text(t) if t == "09:00 local")
            }),
            "html read must keep <u> as an underline run"
        );
        let out = to_html(&doc);
        assert!(out.contains("<s>guess</s>"), "{out}");
        assert!(out.contains("<s>hope</s>"), "{out}");
        assert!(out.contains("<u>09:00 local</u>"), "{out}");
        assert!(
            out.contains("s,del{text-decoration:line-through;color:#134e4a}"),
            "round-trip HTML must keep WeasyPrint s/del strikethrough: {out}"
        );
        assert!(
            out.contains("u{text-decoration:underline}"),
            "round-trip HTML must keep WeasyPrint underline: {out}"
        );
        assert!(
            out.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th fill must not regress: {out}"
        );
        assert!(
            out.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "tasks must not regress: {out}"
        );
        assert!(
            out.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "code chip must not regress: {out}"
        );
        assert!(
            out.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark must not regress: {out}"
        );
        assert!(
            out.contains("img{min-width:24pt;min-height:24pt"),
            "24pt img floor must not regress: {out}"
        );
        assert!(out.contains("padding:.4rem .65rem"), "{out}");
        assert!(
            out.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{out}"
        );
    }

    #[test]
    fn article_css_super_sub_and_landed_weasyprint() {
        let html = to_html(&Document::new());
        assert!(
            html.contains("sup{font-size:.75em;line-height:normal;vertical-align:super}"),
            "sup must keep WeasyPrint 0.75em size, normal leading, and super offset, not a missing UA style: {html}"
        );
        assert!(
            html.contains("sub{font-size:.75em;line-height:normal;vertical-align:sub}"),
            "sub must keep WeasyPrint 0.75em size, normal leading, and sub offset, not a missing UA style: {html}"
        );
        assert!(
            !html.contains("sup{vertical-align:baseline")
                && !html.contains("sub{vertical-align:baseline")
                && !html.contains("sup{font-size:1em")
                && !html.contains("sub{font-size:1em")
                && !html.contains("sup{font-size:inherit")
                && !html.contains("sub{font-size:inherit")
                && !html.contains("sup{line-height:1.45")
                && !html.contains("sub{line-height:1.45")
                && !html.contains("sup{vertical-align:sub")
                && !html.contains("sub{vertical-align:super"),
            "reset/inherited/swapped super/sub hides letter PDF/A and H2O: {html}"
        );
        assert!(
            html.contains("s,del{text-decoration:line-through;color:#134e4a}"),
            "strike must not regress: {html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th fill must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "task lists must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks input[type=checkbox]{appearance:none;-webkit-appearance:none;display:inline-block;width:.9em;height:.9em;margin-right:.4rem;vertical-align:middle;border:1.5pt solid #134e4a;border-radius:2px;background:#fff;accent-color:#134e4a}"),
            "task checkbox CSS boxes must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks input[type=checkbox]:checked{background:#134e4a}"),
            "checked tasks must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem"),
            "cell pad must not regress: {html}"
        );
        assert!(
            html.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{html}"
        );
        assert!(
            html.contains("h2{font-size:1.15rem;line-height:1.2"),
            "{html}"
        );
        assert!(html.contains("font-size:10pt;line-height:1.45"), "{html}");
        assert!(html.contains("p{margin:0 0 .7rem}"), "{html}");
        assert!(html.contains(r#"article{max-width:48rem"#), "{html}");
        assert!(
            html.contains("u{text-decoration:underline}"),
            "underline must not regress: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "inline code chip must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark highlight must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_html_super_and_sub_styles() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("PDF/^A^"),
            "letter.md must keep the PDF/A superscript WeasyPrint paints as <sup>"
        );
        assert!(
            text.contains("H~2~O"),
            "letter.md must keep the H2O subscript WeasyPrint paints as <sub>"
        );
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let body = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p)
                    if p.outline_level.is_none()
                        && p.numbering.is_none()
                        && p.plain_text().contains("PDF/") =>
                {
                    Some(p)
                }
                _ => None,
            })
            .expect("super para");
        assert!(
            body.runs.iter().any(|r| {
                r.style.superscript && matches!(&r.content, RunContent::Text(t) if t == "A")
            }),
            "letter.md IR must keep PDF/^A^ as superscript"
        );
        assert!(
            body.runs.iter().any(|r| {
                r.style.subscript && matches!(&r.content, RunContent::Text(t) if t == "2")
            }),
            "letter.md IR must keep H~2~O as subscript"
        );
        assert!(
            body.runs.iter().any(|r| {
                r.style.strike && matches!(&r.content, RunContent::Text(t) if t == "guess")
            }),
            "strike must not regress"
        );
        let img = first_image(&doc);
        assert!(img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU);
        let t = first_table(&doc);
        assert_eq!(t.rows[0].cells[0].padding.left, CELL_PAD_X_HU);
        assert_eq!(t.rows[0].cells[0].shading, Some(TH_BG));
        assert!(
            numbered_paras(&doc).iter().any(|p| p
                .numbering
                .as_ref()
                .is_some_and(|n| n.format == Some(NumberFormat::Task))),
            "tasks must not regress"
        );
        let html = to_html(&doc);
        assert!(html.contains("PDF/<sup>A</sup>"), "{html}");
        assert!(html.contains("H<sub>2</sub>O"), "{html}");
        assert!(
            html.contains("sup{font-size:.75em;line-height:normal;vertical-align:super}"),
            "letter.md HTML super must look like WeasyPrint, not missing: {html}"
        );
        assert!(
            html.contains("sub{font-size:.75em;line-height:normal;vertical-align:sub}"),
            "letter.md HTML sub must look like WeasyPrint, not missing: {html}"
        );
        assert!(
            !html.contains("sup{vertical-align:baseline")
                && !html.contains("sub{vertical-align:baseline")
                && !html.contains("sup{line-height:1.45")
                && !html.contains("sub{line-height:1.45"),
            "letter.md HTML super/sub must not inherit article 1.45 leading: {html}"
        );
        assert!(html.contains("<s>guess</s>"), "{html}");
        assert!(
            html.contains("s,del{text-decoration:line-through;color:#134e4a}"),
            "strike must not regress: {html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th fill must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "tasks must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks input[type=checkbox]{appearance:none;-webkit-appearance:none;display:inline-block;width:.9em;height:.9em;margin-right:.4rem;vertical-align:middle;border:1.5pt solid #134e4a;border-radius:2px;background:#fff;accent-color:#134e4a}"),
            "task checkbox CSS boxes must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(html.contains("<mark>three hashes</mark>"), "{html}");
        assert!(html.contains(r#"<ul class="tasks">"#), "{html}");
    }

    #[test]
    fn html_read_super_and_sub_roundtrip() {
        let html = b"<article><p>the PDF/<sup>A</sup> bytes match. Wet cargo is H<sub>2</sub>O only.</p></article>";
        let doc = read(html).expect("html");
        let p = first_para(&doc);
        assert!(
            p.runs.iter().any(|r| {
                r.style.superscript && matches!(&r.content, RunContent::Text(t) if t == "A")
            }),
            "html read must keep <sup> as a superscript run"
        );
        assert!(
            p.runs.iter().any(|r| {
                r.style.subscript && matches!(&r.content, RunContent::Text(t) if t == "2")
            }),
            "html read must keep <sub> as a subscript run"
        );
        let out = to_html(&doc);
        assert!(out.contains("<sup>A</sup>"), "{out}");
        assert!(out.contains("<sub>2</sub>"), "{out}");
        assert!(
            out.contains("sup{font-size:.75em;line-height:normal;vertical-align:super}"),
            "round-trip HTML must keep WeasyPrint super size and offset: {out}"
        );
        assert!(
            out.contains("sub{font-size:.75em;line-height:normal;vertical-align:sub}"),
            "round-trip HTML must keep WeasyPrint sub size and offset: {out}"
        );
        assert!(
            out.contains("s,del{text-decoration:line-through;color:#134e4a}"),
            "strike must not regress: {out}"
        );
        assert!(
            out.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th fill must not regress: {out}"
        );
        assert!(
            out.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "tasks must not regress: {out}"
        );
        assert!(
            out.contains("img{min-width:24pt;min-height:24pt"),
            "24pt img floor must not regress: {out}"
        );
        assert!(out.contains("padding:.4rem .65rem"), "{out}");
        assert!(
            out.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{out}"
        );
    }

    #[test]
    fn article_css_strong_em_and_landed_weasyprint() {
        let html = to_html(&Document::new());
        assert!(
            html.contains("strong,b{font-weight:700}"),
            "strong/b must keep letter.html weight 700, not a missing UA bolder: {html}"
        );
        assert!(
            html.contains("em,i{font-style:italic}"),
            "em/i must keep letter.html italic, not a missing UA style: {html}"
        );
        assert!(
            !html.contains("strong{font-weight:400")
                && !html.contains("strong{font-weight:normal")
                && !html.contains("strong{font-weight:bolder")
                && !html.contains("b{font-weight:400")
                && !html.contains("b{font-weight:normal")
                && !html.contains("em{font-style:normal")
                && !html.contains("i{font-style:normal")
                && !html.contains("strong{font-weight:inherit")
                && !html.contains("em{font-style:inherit"),
            "reset/normal/bolder/inherit strong/em hides letter GPU-free / byte-for-byte: {html}"
        );
        assert!(
            html.contains("sup{font-size:.75em;line-height:normal;vertical-align:super}"),
            "super must not regress: {html}"
        );
        assert!(
            html.contains("sub{font-size:.75em;line-height:normal;vertical-align:sub}"),
            "sub must not regress: {html}"
        );
        assert!(
            html.contains("s,del{text-decoration:line-through;color:#134e4a}"),
            "strike must not regress: {html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th fill must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "task lists must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks input[type=checkbox]{appearance:none;-webkit-appearance:none;display:inline-block;width:.9em;height:.9em;margin-right:.4rem;vertical-align:middle;border:1.5pt solid #134e4a;border-radius:2px;background:#fff;accent-color:#134e4a}"),
            "task checkbox CSS boxes must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks input[type=checkbox]:checked{background:#134e4a}"),
            "checked tasks must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem"),
            "cell pad must not regress: {html}"
        );
        assert!(
            html.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{html}"
        );
        assert!(
            html.contains("h2{font-size:1.15rem;line-height:1.2"),
            "{html}"
        );
        assert!(html.contains("font-size:10pt;line-height:1.45"), "{html}");
        assert!(html.contains("p{margin:0 0 .7rem}"), "{html}");
        assert!(html.contains(r#"article{max-width:48rem"#), "{html}");
        assert!(
            html.contains("u{text-decoration:underline}"),
            "underline must not regress: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "inline code chip must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark highlight must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_html_strong_and_em_styles() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("**GPU-free**"),
            "letter.md must keep the bold WeasyPrint paints as <strong>"
        );
        assert!(
            text.contains("_byte-for-byte_"),
            "letter.md must keep the italic WeasyPrint paints as <em>"
        );
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let body = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p)
                    if p.outline_level.is_none()
                        && p.numbering.is_none()
                        && p.plain_text().contains("GPU-free") =>
                {
                    Some(p)
                }
                _ => None,
            })
            .expect("strong/em para");
        assert!(
            body.runs.iter().any(|r| {
                r.style.bold && matches!(&r.content, RunContent::Text(t) if t == "GPU-free")
            }),
            "letter.md IR must keep **GPU-free** as bold"
        );
        assert!(
            body.runs.iter().any(|r| {
                r.style.italic && matches!(&r.content, RunContent::Text(t) if t == "byte-for-byte")
            }),
            "letter.md IR must keep _byte-for-byte_ as italic"
        );
        assert!(
            body.runs.iter().any(|r| {
                r.style.superscript && matches!(&r.content, RunContent::Text(t) if t == "A")
            }),
            "super must not regress"
        );
        assert!(
            body.runs.iter().any(|r| {
                r.style.subscript && matches!(&r.content, RunContent::Text(t) if t == "2")
            }),
            "sub must not regress"
        );
        let img = first_image(&doc);
        assert!(img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU);
        let t = first_table(&doc);
        assert_eq!(t.rows[0].cells[0].padding.left, CELL_PAD_X_HU);
        assert_eq!(t.rows[0].cells[0].shading, Some(TH_BG));
        assert!(
            numbered_paras(&doc).iter().any(|p| p
                .numbering
                .as_ref()
                .is_some_and(|n| n.format == Some(NumberFormat::Task))),
            "tasks must not regress"
        );
        let html = to_html(&doc);
        assert!(html.contains("<strong>GPU-free</strong>"), "{html}");
        assert!(html.contains("<em>byte-for-byte</em>"), "{html}");
        assert!(
            html.contains("strong,b{font-weight:700}"),
            "letter.md HTML strong/b must look like WeasyPrint, not missing: {html}"
        );
        assert!(
            html.contains("em,i{font-style:italic}"),
            "letter.md HTML em/i must look like WeasyPrint, not missing: {html}"
        );
        assert!(
            !html.contains("strong{font-weight:bolder")
                && !html.contains("em{font-style:normal")
                && !html.contains("strong{font-weight:400"),
            "letter.md HTML strong/em must not keep UA bolder/normal: {html}"
        );
        assert!(html.contains("PDF/<sup>A</sup>"), "{html}");
        assert!(html.contains("H<sub>2</sub>O"), "{html}");
        assert!(
            html.contains("sup{font-size:.75em;line-height:normal;vertical-align:super}"),
            "super must not regress: {html}"
        );
        assert!(
            html.contains("sub{font-size:.75em;line-height:normal;vertical-align:sub}"),
            "sub must not regress: {html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th fill must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "tasks must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks input[type=checkbox]{appearance:none;-webkit-appearance:none;display:inline-block;width:.9em;height:.9em;margin-right:.4rem;vertical-align:middle;border:1.5pt solid #134e4a;border-radius:2px;background:#fff;accent-color:#134e4a}"),
            "task checkbox CSS boxes must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(html.contains("<mark>three hashes</mark>"), "{html}");
        assert!(html.contains(r#"<ul class="tasks">"#), "{html}");
    }

    #[test]
    fn html_read_strong_and_em_roundtrip() {
        let html = b"<article><p>one <strong>two</strong> three <em>four</em>. Also <b>GPU-free</b> and <i>byte-for-byte</i>.</p></article>";
        let doc = read(html).expect("html");
        let p = first_para(&doc);
        assert!(
            p.runs.iter().any(|r| {
                r.style.bold && matches!(&r.content, RunContent::Text(t) if t == "two")
            }),
            "html read must keep <strong> as a bold run"
        );
        assert!(
            p.runs.iter().any(|r| {
                r.style.italic && matches!(&r.content, RunContent::Text(t) if t == "four")
            }),
            "html read must keep <em> as an italic run"
        );
        assert!(
            p.runs.iter().any(|r| {
                r.style.bold && matches!(&r.content, RunContent::Text(t) if t == "GPU-free")
            }),
            "html read must keep <b> as a bold run"
        );
        assert!(
            p.runs.iter().any(|r| {
                r.style.italic && matches!(&r.content, RunContent::Text(t) if t == "byte-for-byte")
            }),
            "html read must keep <i> as an italic run"
        );
        let out = to_html(&doc);
        assert!(out.contains("<strong>two</strong>"), "{out}");
        assert!(out.contains("<em>four</em>"), "{out}");
        assert!(out.contains("<strong>GPU-free</strong>"), "{out}");
        assert!(out.contains("<em>byte-for-byte</em>"), "{out}");
        assert!(
            out.contains("strong,b{font-weight:700}"),
            "round-trip HTML must keep WeasyPrint strong/b weight 700: {out}"
        );
        assert!(
            out.contains("em,i{font-style:italic}"),
            "round-trip HTML must keep WeasyPrint em/i italic: {out}"
        );
        assert!(
            out.contains("sup{font-size:.75em;line-height:normal;vertical-align:super}"),
            "super must not regress: {out}"
        );
        assert!(
            out.contains("sub{font-size:.75em;line-height:normal;vertical-align:sub}"),
            "sub must not regress: {out}"
        );
        assert!(
            out.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th fill must not regress: {out}"
        );
        assert!(
            out.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "tasks must not regress: {out}"
        );
        assert!(
            out.contains("img{min-width:24pt;min-height:24pt"),
            "24pt img floor must not regress: {out}"
        );
        assert!(out.contains("padding:.4rem .65rem"), "{out}");
        assert!(
            out.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{out}"
        );
    }

    #[test]
    fn article_css_blockquote_bar_and_landed_weasyprint() {
        let html = to_html(&Document::new());
        assert!(
            html.contains(
                "blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem;border-left:4px solid #134e4a;color:#134e4a}"
            ),
            "blockquote left bar must stay letter.html 4px solid #134e4a (not a missing UA quote): {html}"
        );
        assert!(
            !html.contains("blockquote{border-left:none")
                && !html.contains("blockquote{border-left:0")
                && !html.contains("blockquote{border-left:1px")
                && !html.contains("blockquote{border-left:3px")
                && !html.contains("blockquote{border-left:4px solid #000")
                && !html.contains("blockquote{border-left:4px solid #ccc")
                && !html.contains("blockquote{border-left:4px solid #999")
                && !html.contains("blockquote{margin:1em 40px")
                && !html.contains("blockquote{border:none"),
            "UA/reset/1px/gray quote bar is not WeasyPrint letter input: {html}"
        );
        assert_eq!(QUOTE_BAR_W_HU, 300, "4px at 96dpi must stay 300 HU");
        assert_eq!(
            QUOTE_FG, TH_FG,
            "quote bar and th text share letter teal #134e4a"
        );
        assert!(
            html.contains("strong,b{font-weight:700}"),
            "strong/em must not regress: {html}"
        );
        assert!(
            html.contains("em,i{font-style:italic}"),
            "strong/em must not regress: {html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th fill must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "tasks must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks input[type=checkbox]{appearance:none;-webkit-appearance:none;display:inline-block;width:.9em;height:.9em;margin-right:.4rem;vertical-align:middle;border:1.5pt solid #134e4a;border-radius:2px;background:#fff;accent-color:#134e4a}"),
            "task checkbox CSS boxes must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks input[type=checkbox]:checked{background:#134e4a}"),
            "checked tasks must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem"),
            "cell pad must not regress: {html}"
        );
        assert!(
            html.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{html}"
        );
        assert!(
            html.contains("h2{font-size:1.15rem;line-height:1.2"),
            "{html}"
        );
        assert!(html.contains("font-size:10pt;line-height:1.45"), "{html}");
        assert!(html.contains("p{margin:0 0 .7rem}"), "{html}");
        assert!(html.contains(r#"article{max-width:48rem"#), "{html}");
        assert!(
            html.contains("s,del{text-decoration:line-through;color:#134e4a}"),
            "strike must not regress: {html}"
        );
        assert!(
            html.contains("u{text-decoration:underline}"),
            "underline must not regress: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "inline code chip must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark highlight must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_html_blockquote_bar() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("> Replay is evidence"),
            "letter.md must keep the blockquote WeasyPrint paints with a 4px left bar"
        );
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let quote = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) if p.quote => Some(p),
                _ => None,
            })
            .expect("quote");
        assert!(quote.plain_text().contains("Replay is evidence"));
        assert_eq!(quote.space_before, QUOTE_BEFORE_HU);
        assert_eq!(quote.space_after, QUOTE_AFTER_HU);
        assert!(
            quote.runs.iter().any(|r| {
                r.style.color == QUOTE_FG
                    && matches!(&r.content, RunContent::Text(t) if t.contains("Replay"))
            }),
            "letter.md quote text must stay #134e4a like the 4px bar"
        );
        let t = first_table(&doc);
        assert_eq!(t.rows[0].cells[0].shading, Some(TH_BG));
        assert!(
            numbered_paras(&doc).iter().any(|p| p
                .numbering
                .as_ref()
                .is_some_and(|n| n.format == Some(NumberFormat::Task))),
            "tasks must not regress"
        );
        let img = first_image(&doc);
        assert!(img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU);
        let html = to_html(&doc);
        assert!(html.contains("<blockquote>Replay is evidence"), "{html}");
        assert!(
            html.contains(
                "blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem;border-left:4px solid #134e4a;color:#134e4a}"
            ),
            "letter.md HTML quote bar must match letter.html 4px #134e4a: {html}"
        );
        assert!(
            !html.contains("blockquote{border-left:none")
                && !html.contains("blockquote{border-left:1px")
                && !html.contains("blockquote{margin:1em 40px"),
            "letter.md HTML must not keep a UA quote: {html}"
        );
        assert!(
            html.contains("strong,b{font-weight:700}") && html.contains("em,i{font-style:italic}"),
            "strong/em must not regress: {html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th fill must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "tasks must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
    }

    #[test]
    fn html_read_blockquote_keeps_weasyprint_bar_color() {
        let html = b"<blockquote>Replay is evidence. Same fonts, same bytes.</blockquote>";
        let doc = read(html).expect("html");
        let p = first_para(&doc);
        assert!(p.quote);
        assert_eq!(p.space_before, QUOTE_BEFORE_HU);
        assert_eq!(p.space_after, QUOTE_AFTER_HU);
        assert!(
            p.runs.iter().any(|r| r.style.color == QUOTE_FG),
            "html read must keep blockquote color #134e4a"
        );
        assert_ne!(
            p.runs[0].style.color,
            Color::BLACK,
            "quote text must not stay body black; letter CSS is #134e4a"
        );
        let out = to_html(&doc);
        assert!(out.contains("<blockquote>Replay is evidence"), "{out}");
        assert!(
            out.contains(
                "blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem;border-left:4px solid #134e4a;color:#134e4a}"
            ),
            "round-trip HTML must keep the 4px #134e4a quote bar: {out}"
        );
        assert!(
            out.contains("strong,b{font-weight:700}"),
            "strong/em must not regress: {out}"
        );
        assert!(
            out.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th fill must not regress: {out}"
        );
        assert!(
            out.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "tasks must not regress: {out}"
        );
        assert!(
            out.contains("img{min-width:24pt;min-height:24pt"),
            "24pt img floor must not regress: {out}"
        );
    }

    #[test]
    fn article_css_code_pre_font_family_and_landed_weasyprint() {
        let html = to_html(&Document::new());
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "code must keep letter.html ui-monospace/Consolas, not generic serif: {html}"
        );
        assert!(
            html.contains("pre{background:#ccfbf1;padding:.75rem 1rem;margin:.6rem 0 1rem;border-radius:4px;overflow:auto;font-family:ui-monospace,Consolas,monospace}"),
            "pre must keep the same ui-monospace/Consolas stack as code: {html}"
        );
        assert!(
            html.contains("pre code{background:transparent;padding:0;color:#134e4a;font-family:ui-monospace,Consolas,monospace}"),
            "pre code must keep Consolas after dropping the chip fill: {html}"
        );
        assert!(
            html.contains("font-family:ui-monospace,Consolas,monospace"),
            "WeasyPrint letter input must name Consolas, not a UA serif: {html}"
        );
        assert!(
            !html.contains("code{font-family:serif")
                && !html.contains("pre{font-family:serif")
                && !html.contains("code{font-family:Times")
                && !html.contains("pre{font-family:Times")
                && !html.contains(r#"code{font-family:"Times"#)
                && !html.contains(r#"pre{font-family:"Times"#)
                && !html.contains(r#"code{font-family:"Segoe UI""#)
                && !html.contains(r#"pre{font-family:"Segoe UI""#),
            "code/pre must not inherit article serif or Segoe UI: {html}"
        );
        assert!(
            html.contains(
                "blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem;border-left:4px solid #134e4a;color:#134e4a}"
            ),
            "quote bar must not regress: {html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th teal must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "tasks must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks input[type=checkbox]{appearance:none;-webkit-appearance:none;display:inline-block;width:.9em;height:.9em;margin-right:.4rem;vertical-align:middle;border:1.5pt solid #134e4a;border-radius:2px;background:#fff;accent-color:#134e4a}"),
            "task CSS boxes must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_html_code_pre_font_family_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let body = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p)
                    if p.outline_level.is_none()
                        && p.numbering.is_none()
                        && p.plain_text().contains("prove") =>
                {
                    Some(p)
                }
                _ => None,
            })
            .expect("body");
        assert!(
            body.runs.iter().any(|r| {
                r.style.code
                    && r.style.font.as_deref() == Some(CODE_FONT)
                    && matches!(&r.content, RunContent::Text(t) if t == "prove")
            }),
            "letter.md `prove` must be Consolas, not article serif"
        );
        let pre = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) if p.code_block => Some(p),
                _ => None,
            })
            .expect("pre");
        assert!(
            pre.runs
                .iter()
                .any(|r| r.style.font.as_deref() == Some(CODE_FONT)),
            "letter.md fence must be Consolas: {:?}",
            pre.runs
                .iter()
                .map(|r| r.style.font.clone())
                .collect::<Vec<_>>()
        );
        let html = to_html(&doc);
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(html.contains("<pre><code>"), "{html}");
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "letter.md HTML code must match letter.html ui-monospace/Consolas: {html}"
        );
        assert!(
            html.contains("pre{background:#ccfbf1;padding:.75rem 1rem;margin:.6rem 0 1rem;border-radius:4px;overflow:auto;font-family:ui-monospace,Consolas,monospace}"),
            "letter.md HTML pre must match letter.html Consolas stack: {html}"
        );
        assert!(
            !html.contains("code{font-family:serif") && !html.contains("pre{font-family:serif"),
            "letter.md HTML code/pre must not be generic serif: {html}"
        );
        assert!(
            html.contains(
                "blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem;border-left:4px solid #134e4a;color:#134e4a}"
            ),
            "quote bar must not regress: {html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th teal must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "tasks must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
    }

    #[test]
    fn article_css_readable_measure_and_landed_weasyprint() {
        let html = to_html(&Document::new());
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48);
        assert_ne!(
            ARTICLE_MAX_WIDTH_REM, 100,
            "100rem / 100% is a full-bleed column, not letter.html"
        );
        assert_eq!(
            ARTICLE_RULE,
            r#"article{max-width:48rem;margin:2rem auto;padding:2.5rem 2.75rem;background:#fff;border:1px solid #e2e8f0;font-family:"Segoe UI",Roboto,Helvetica,Arial,sans-serif;font-size:10pt;line-height:1.45;color:#134e4a}"#
        );
        assert!(
            html.contains(ARTICLE_RULE),
            "article must keep letter.html 48rem measure, not a full-bleed sparse column: {html}"
        );
        assert!(
            html.contains(PAGE_RULE),
            "print must pin @page to letter.html 2.5rem 2.75rem, not UA 75px: {html}"
        );
        assert!(
            html.contains(PRINT_RULE),
            "print must drop the screen card chrome so @page is the inset: {html}"
        );
        assert!(
            html.contains(BODY_RULE),
            "body must keep the slate page and letter teal type around the paper card: {html}"
        );
        assert_eq!(BODY_RULE, "body{margin:0;background:#f8fafc;color:#134e4a}");
        assert!(
            html.contains("body{margin:0;background:#f8fafc;color:#134e4a}"),
            "body type must stay letter teal #134e4a, not UA black or slate #111827: {html}"
        );
        assert!(
            html.contains(";line-height:1.45;color:#134e4a}"),
            "article must pin color:#134e4a so WeasyPrint does not paint UA black: {html}"
        );
        assert!(
            !html.contains("color:#111827")
                && !html.contains("body{margin:0;background:#f8fafc;color:#000")
                && !html.contains("article{color:#000")
                && !html.contains("color:black"),
            "slate #111827 / UA black body type is not letter.html WeasyPrint input: {html}"
        );
        assert!(
            html.contains(
                "article{max-width:48rem;margin:2rem auto;padding:2.5rem 2.75rem;background:#fff;border:1px solid #e2e8f0"
            ),
            "readable measure must stay 48rem centered with letter padding: {html}"
        );
        assert!(
            html.contains("font-size:10pt;line-height:1.45"),
            "10pt / 1.45 is the type the 48rem column is sized for: {html}"
        );
        assert!(
            !html.contains("article{max-width:100%")
                && !html.contains("article{max-width:none")
                && !html.contains("article{width:100%")
                && !html.contains("article{margin:0;")
                && !html.contains("article{padding:0")
                && !html.contains("article{max-width:48rem}"),
            "full-bleed / unpadded article is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "Consolas must not regress: {html}"
        );
        assert!(
            html.contains("pre{background:#ccfbf1;padding:.75rem 1rem;margin:.6rem 0 1rem;border-radius:4px;overflow:auto;font-family:ui-monospace,Consolas,monospace}"),
            "pre Consolas must not regress: {html}"
        );
        assert!(
            html.contains(
                "blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem;border-left:4px solid #134e4a;color:#134e4a}"
            ),
            "quote bar must not regress: {html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th teal must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem"),
            "cell pad must not regress: {html}"
        );
        assert!(
            html.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{html}"
        );
        assert!(
            html.contains("h2{font-size:1.15rem;line-height:1.2"),
            "{html}"
        );
        assert!(html.contains("p{margin:0 0 .7rem}"), "{html}");
        assert!(
            html.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "tasks must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_html_readable_measure_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let t = first_table(&doc);
        assert_eq!(
            t.rows[0].cells[0].shading,
            Some(TH_BG),
            "th must not regress"
        );
        let quote = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) if p.quote => Some(p),
                _ => None,
            })
            .expect("quote");
        assert!(quote.runs.iter().any(|r| r.style.color == QUOTE_FG));
        assert!(
            doc.sections[0].body.iter().any(|b| match b {
                Block::Paragraph(p) => p.runs.iter().any(|r| {
                    r.style.code
                        && r.style.font.as_deref() == Some(CODE_FONT)
                        && matches!(&r.content, RunContent::Text(t) if t == "prove")
                }),
                _ => false,
            }),
            "letter.md `prove` must stay Consolas"
        );
        let html = to_html(&doc);
        assert!(html.contains("<article"), "{html}");
        assert!(
            html.contains(ARTICLE_RULE),
            "letter.md HTML must keep letter.html 48rem measure, not a full-bleed column: {html}"
        );
        assert!(
            html.contains(
                "article{max-width:48rem;margin:2rem auto;padding:2.5rem 2.75rem;background:#fff;border:1px solid #e2e8f0"
            ),
            "letter.md HTML readable measure must stay 48rem centered: {html}"
        );
        assert!(
            !html.contains("article{max-width:100%")
                && !html.contains("article{max-width:none")
                && !html.contains("article{width:100%")
                && !html.contains("article{margin:0;")
                && !html.contains("article{padding:0"),
            "letter.md HTML must not be a full-bleed sparse column: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "letter.md HTML Consolas must not regress: {html}"
        );
        assert!(
            html.contains(
                "blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem;border-left:4px solid #134e4a;color:#134e4a}"
            ),
            "letter.md HTML quote bar must not regress: {html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "letter.md HTML th teal must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "letter.md HTML 24pt img floor must not regress: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(html.contains("<blockquote>Replay is evidence"), "{html}");
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(html.contains("<th><strong>Hop</strong></th>"), "{html}");
        assert!(
            html.contains(PAGE_RULE),
            "letter.md HTML print must pin @page, not WeasyPrint UA 75px: {html}"
        );
        assert!(
            html.contains(BODY_RULE)
                && html.contains("body{margin:0;background:#f8fafc;color:#134e4a}"),
            "letter.md HTML body type must stay #134e4a, not UA black: {html}"
        );
        assert!(
            !html.contains("color:#111827"),
            "letter.md HTML must not keep slate #111827 body type: {html}"
        );
    }

    #[test]
    fn article_css_print_page_margins_and_landed_weasyprint() {
        let html = to_html(&Document::new());
        assert_eq!(PAGE_RULE, "@page{size:A4;margin:2.5rem 2.75rem}");
        assert_eq!(
            PRINT_RULE,
            "@media print{body{background:#fff}article{margin:0 auto;border:none;padding:0}}"
        );
        assert_ne!(
            PAGE_RULE, "@page{margin:75px}",
            "WeasyPrint UA @page margin 75px is not letter.html"
        );
        assert!(
            html.contains(PAGE_RULE),
            "article CSS must pin @page A4 / 2.5rem 2.75rem matching letter.html padding, not UA 75px: {html}"
        );
        assert!(
            html.contains(PRINT_RULE),
            "print must use @page as the letter.html inset, not a second card pad: {html}"
        );
        assert!(
            html.contains("@page{size:A4;margin:2.5rem 2.75rem}"),
            "print page margins must match letter.html article padding 2.5rem 2.75rem: {html}"
        );
        assert!(
            !html.contains("@page{margin:75px")
                && !html.contains("@page{margin:0}")
                && !html.contains("@page{margin:0.75in")
                && !html.contains("@page{margin:.75in")
                && !html.contains("@page{margin:2cm"),
            "UA/reset @page is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(ARTICLE_RULE),
            "48rem measure must not regress: {html}"
        );
        assert!(
            html.contains(
                "article{max-width:48rem;margin:2rem auto;padding:2.5rem 2.75rem;background:#fff;border:1px solid #e2e8f0"
            ),
            "readable measure must stay 48rem centered with letter padding: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "Consolas must not regress: {html}"
        );
        assert!(
            html.contains("pre{background:#ccfbf1;padding:.75rem 1rem;margin:.6rem 0 1rem;border-radius:4px;overflow:auto;font-family:ui-monospace,Consolas,monospace}"),
            "pre Consolas must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem"),
            "cell pad must not regress: {html}"
        );
        assert!(
            html.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{html}"
        );
        assert!(
            html.contains("h2{font-size:1.15rem;line-height:1.2"),
            "{html}"
        );
        assert!(html.contains("p{margin:0 0 .7rem}"), "{html}");
        assert!(
            html.contains(
                "blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem;border-left:4px solid #134e4a;color:#134e4a}"
            ),
            "quote bar must not regress: {html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th teal must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "tasks must not regress: {html}"
        );
        assert!(
            html.contains("s,del{text-decoration:line-through;color:#134e4a}"),
            "s/del must not regress: {html}"
        );
        assert!(html.contains("u{text-decoration:underline}"), "{html}");
        assert!(
            html.contains("strong,b{font-weight:700}") && html.contains("em,i{font-style:italic}"),
            "strong/em must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE)
                && html.contains("body{margin:0;background:#f8fafc;color:#134e4a}"),
            "body type must stay letter teal #134e4a: {html}"
        );
        assert!(
            html.contains(";line-height:1.45;color:#134e4a}"),
            "article must pin color:#134e4a, not inherit UA black: {html}"
        );
        assert!(
            !html.contains("color:#111827"),
            "slate #111827 body type is not WeasyPrint letter input: {html}"
        );
    }

    #[test]
    fn letter_md_html_print_page_margins_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let t = first_table(&doc);
        assert_eq!(
            t.rows[0].cells[0].shading,
            Some(TH_BG),
            "th must not regress"
        );
        assert!(
            doc.sections[0].body.iter().any(|b| match b {
                Block::Paragraph(p) => p.runs.iter().any(|r| {
                    r.style.code
                        && r.style.font.as_deref() == Some(CODE_FONT)
                        && matches!(&r.content, RunContent::Text(t) if t == "prove")
                }),
                _ => false,
            }),
            "letter.md `prove` must stay Consolas"
        );
        let html = to_html(&doc);
        assert!(html.contains("<article"), "{html}");
        assert!(
            html.contains(PAGE_RULE),
            "letter.md HTML must pin @page A4 / 2.5rem 2.75rem matching letter.html, not UA 75px: {html}"
        );
        assert!(
            html.contains(PRINT_RULE),
            "letter.md HTML print must not stack UA 75px on the screen card: {html}"
        );
        assert!(
            !html.contains("@page{margin:75px")
                && !html.contains("@page{margin:0}")
                && !html.contains("@page{margin:0.75in"),
            "letter.md HTML must not keep WeasyPrint UA page margins: {html}"
        );
        assert!(
            html.contains(ARTICLE_RULE),
            "48rem measure must not regress: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "Consolas must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(html.contains("<blockquote>Replay is evidence"), "{html}");
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(html.contains("<th><strong>Hop</strong></th>"), "{html}");
        assert!(
            html.contains(BODY_RULE)
                && html.contains("body{margin:0;background:#f8fafc;color:#134e4a}"),
            "letter.md HTML body type must stay #134e4a: {html}"
        );
        assert!(
            !html.contains("color:#111827"),
            "letter.md HTML must not keep slate #111827: {html}"
        );
    }

    #[test]
    fn article_css_body_color_and_landed_weasyprint() {
        let html = to_html(&Document::new());
        assert_eq!(
            BODY_FG,
            Color {
                r: 19,
                g: 78,
                b: 74,
                a: 255
            }
        );
        assert_eq!(BODY_FG, QUOTE_FG);
        assert_eq!(BODY_FG, TH_FG);
        assert_eq!(BODY_RULE, "body{margin:0;background:#f8fafc;color:#134e4a}");
        assert_ne!(
            BODY_RULE, "body{margin:0;background:#f8fafc;color:#111827}",
            "slate #111827 is not letter teal body type"
        );
        assert_eq!(
            ARTICLE_RULE,
            r#"article{max-width:48rem;margin:2rem auto;padding:2.5rem 2.75rem;background:#fff;border:1px solid #e2e8f0;font-family:"Segoe UI",Roboto,Helvetica,Arial,sans-serif;font-size:10pt;line-height:1.45;color:#134e4a}"#
        );
        assert!(
            html.contains(BODY_RULE),
            "body must pin letter teal #134e4a, not UA black: {html}"
        );
        assert!(
            html.contains("body{margin:0;background:#f8fafc;color:#134e4a}"),
            "body color must stay letter.html #134e4a: {html}"
        );
        assert!(
            html.contains(ARTICLE_RULE),
            "article must pin color:#134e4a on the 48rem column: {html}"
        );
        assert!(
            html.contains(";line-height:1.45;color:#134e4a}"),
            "article type must stay #134e4a so WeasyPrint does not paint UA black: {html}"
        );
        assert!(
            !html.contains("color:#111827")
                && !html.contains("body{margin:0;background:#f8fafc;color:#000")
                && !html.contains("body{margin:0;background:#f8fafc;color:#000000")
                && !html.contains("article{color:#000")
                && !html.contains("color:black")
                && !html.contains("color:initial")
                && !html.contains("color:inherit}"),
            "UA black / slate #111827 / inherit-reset body type is not WeasyPrint letter input: {html}"
        );
        assert_eq!(PAGE_RULE, "@page{size:A4;margin:2.5rem 2.75rem}");
        assert!(
            html.contains(PAGE_RULE),
            "@page A4 2.5rem/2.75rem must not regress: {html}"
        );
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48);
        assert!(
            html.contains(r#"article{max-width:48rem"#),
            "48rem measure must not regress: {html}"
        );
        assert_eq!(CODE_FONT, "Consolas");
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "Consolas must not regress: {html}"
        );
        assert!(
            html.contains("pre{background:#ccfbf1;padding:.75rem 1rem;margin:.6rem 0 1rem;border-radius:4px;overflow:auto;font-family:ui-monospace,Consolas,monospace}"),
            "pre Consolas must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem"),
            "cell pad must not regress: {html}"
        );
        assert!(
            html.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{html}"
        );
        assert!(
            html.contains("h2{font-size:1.15rem;line-height:1.2"),
            "{html}"
        );
        assert!(html.contains("p{margin:0 0 .7rem}"), "{html}");
        assert!(
            html.contains(
                "blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem;border-left:4px solid #134e4a;color:#134e4a}"
            ),
            "quote bar must not regress: {html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th teal must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "tasks must not regress: {html}"
        );
        assert!(
            html.contains("s,del{text-decoration:line-through;color:#134e4a}"),
            "s/del must not regress: {html}"
        );
        assert!(html.contains("u{text-decoration:underline}"), "{html}");
        assert!(
            html.contains("strong,b{font-weight:700}") && html.contains("em,i{font-style:italic}"),
            "strong/em must not regress: {html}"
        );
        assert!(
            html.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"),
            "hr must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE) && html.contains("a{color:#0d9488;text-decoration:underline}"),
            "link must not regress: {html}"
        );
        assert!(
            html.contains(LINK_HOVER_RULE) && html.contains("a:hover{color:#134e4a}"),
            "link hover must not regress: {html}"
        );
        assert!(
            html.contains(PRINT_RULE),
            "print @page inset must not regress: {html}"
        );
    }

    #[test]
    fn html_read_body_uses_weasyprint_teal() {
        let doc = read(b"<article><p>The shipment left Busan.</p></article>").expect("html");
        let p = first_para(&doc);
        assert!(
            p.runs.iter().any(|r| {
                r.style.color == BODY_FG
                    && matches!(&r.content, RunContent::Text(t) if t.contains("shipment"))
            }),
            "html body text must stay letter CSS #134e4a, not UA black: {:?}",
            p.runs.iter().map(|r| r.style.color).collect::<Vec<_>>()
        );
        assert!(
            p.runs
                .iter()
                .filter(|r| matches!(r.content, RunContent::Text(_)))
                .all(|r| r.style.color != Color::BLACK),
            "html body text must not keep Color::BLACK: {:?}",
            p.runs.iter().map(|r| r.style.color).collect::<Vec<_>>()
        );
        let out = to_html(&doc);
        assert!(out.contains(BODY_RULE), "{out}");
        assert!(out.contains(ARTICLE_RULE), "{out}");
        assert!(
            out.contains("body{margin:0;background:#f8fafc;color:#134e4a}"),
            "round-trip HTML body type must stay #134e4a: {out}"
        );
        assert!(
            !out.contains("color:#111827"),
            "round-trip HTML must not keep slate #111827: {out}"
        );
        assert!(out.contains(PAGE_RULE), "{out}");
        assert!(out.contains(r#"article{max-width:48rem"#), "{out}");
        assert!(
            out.contains("font-family:ui-monospace,Consolas,monospace"),
            "{out}"
        );
        assert!(out.contains("img{min-width:24pt;min-height:24pt"), "{out}");
        assert!(out.contains(LINK_HOVER_RULE), "{out}");
        assert!(
            out.contains(BODY_RULE) && out.contains("background:#f8fafc"),
            "{out}"
        );
    }

    #[test]
    fn letter_md_html_body_color_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let body = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p)
                    if p.outline_level.is_none()
                        && p.numbering.is_none()
                        && p.plain_text().contains("shipment") =>
                {
                    Some(p)
                }
                _ => None,
            })
            .expect("body");
        assert!(
            body.runs.iter().any(|r| {
                r.style.color == BODY_FG
                    && !r.style.code
                    && matches!(&r.content, RunContent::Text(t) if t.contains("shipment"))
            }),
            "letter.md body text must stay letter CSS #134e4a, not UA black"
        );
        assert!(
            body.runs.iter().any(|r| {
                r.style.code
                    && r.style.font.as_deref() == Some(CODE_FONT)
                    && matches!(&r.content, RunContent::Text(t) if t == "prove")
            }),
            "letter.md `prove` must stay Consolas"
        );
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let t = first_table(&doc);
        assert_eq!(
            t.rows[0].cells[0].shading,
            Some(TH_BG),
            "th must not regress"
        );
        let html = to_html(&doc);
        assert!(
            html.contains(BODY_RULE),
            "letter.md HTML body type must stay #134e4a, not UA black: {html}"
        );
        assert!(
            html.contains("body{margin:0;background:#f8fafc;color:#134e4a}"),
            "letter.md HTML body color must match letter.html #134e4a: {html}"
        );
        assert!(
            html.contains(ARTICLE_RULE),
            "letter.md HTML article must pin color:#134e4a on the 48rem column: {html}"
        );
        assert!(
            html.contains(";line-height:1.45;color:#134e4a}"),
            "letter.md HTML article type must stay #134e4a: {html}"
        );
        assert!(
            !html.contains("color:#111827")
                && !html.contains("body{margin:0;background:#f8fafc;color:#000")
                && !html.contains("color:black"),
            "letter.md HTML must not keep slate #111827 or UA black: {html}"
        );
        assert!(
            html.contains(PAGE_RULE),
            "@page A4 2.5rem/2.75rem must not regress: {html}"
        );
        assert!(
            html.contains(r#"article{max-width:48rem"#),
            "48rem measure must not regress: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "Consolas must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(html.contains("<blockquote>Replay is evidence"), "{html}");
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(html.contains("<th><strong>Hop</strong></th>"), "{html}");
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th teal must not regress: {html}"
        );
        assert!(
            html.contains(
                "blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem;border-left:4px solid #134e4a;color:#134e4a}"
            ),
            "quote bar must not regress: {html}"
        );
        assert!(
            html.contains(LINK_HOVER_RULE) && html.contains("a:hover{color:#134e4a}"),
            "link hover must not regress: {html}"
        );
    }

    #[test]
    fn article_css_link_hover_and_landed_weasyprint() {
        let html = to_html(&Document::new());
        assert_eq!(LINK_RULE, "a{color:#0d9488;text-decoration:underline}");
        assert_eq!(LINK_HOVER_RULE, "a:hover{color:#134e4a}");
        assert_ne!(
            LINK_HOVER_RULE, "a:hover{color:#0d9488}",
            "hover matching rest color is not a rest/hover pair"
        );
        assert_ne!(
            LINK_HOVER_RULE, "a:hover{color:blue}",
            "UA blue hover is not WeasyPrint letter input"
        );
        assert!(
            html.contains(LINK_RULE),
            "links must stay #0d9488 underlined: {html}"
        );
        assert!(
            html.contains(LINK_HOVER_RULE),
            "link hover must rest to letter teal #134e4a, not UA red/blue: {html}"
        );
        assert!(
            html.contains("a{color:#0d9488;text-decoration:underline}a:visited{color:#0d9488}a:hover{color:#134e4a}a:focus{color:#134e4a}"),
            "hover must sit next to the rest link rule: {html}"
        );
        assert!(
            !html.contains("a:hover{color:blue")
                && !html.contains("a:hover{color:red")
                && !html.contains("a:hover{color:#000")
                && !html.contains("a:hover{color:#111827")
                && !html.contains("a:hover{text-decoration:none")
                && !html.contains("a:hover{color:#0d9488"),
            "UA/reset hover is not WeasyPrint letter input: {html}"
        );
        assert_eq!(BODY_RULE, "body{margin:0;background:#f8fafc;color:#134e4a}");
        assert!(
            html.contains(BODY_RULE)
                && html.contains("body{margin:0;background:#f8fafc;color:#134e4a}"),
            "body color must stay letter teal #134e4a: {html}"
        );
        assert!(
            html.contains("background:#f8fafc")
                && html.contains("article{max-width:48rem")
                && html.contains("background:#fff"),
            "slate page + paper card background must not regress: {html}"
        );
        assert_eq!(PAGE_RULE, "@page{size:A4;margin:2.5rem 2.75rem}");
        assert!(
            html.contains(PAGE_RULE),
            "@page A4 2.5rem/2.75rem must not regress: {html}"
        );
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48);
        assert!(
            html.contains(ARTICLE_RULE) && html.contains(r#"article{max-width:48rem"#),
            "48rem measure must not regress: {html}"
        );
        assert_eq!(CODE_FONT, "Consolas");
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "Consolas must not regress: {html}"
        );
        assert!(
            html.contains("pre{background:#ccfbf1;padding:.75rem 1rem;margin:.6rem 0 1rem;border-radius:4px;overflow:auto;font-family:ui-monospace,Consolas,monospace}"),
            "pre Consolas must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"),
            "hr color must stay letter #134e4a: {html}"
        );
        assert!(
            !html.contains("color:#111827"),
            "slate #111827 body type is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(PRINT_RULE),
            "print @page inset must not regress: {html}"
        );
        assert!(
            html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem"),
            "cell pad must not regress: {html}"
        );
        assert!(
            html.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{html}"
        );
        assert!(
            html.contains("h2{font-size:1.15rem;line-height:1.2"),
            "{html}"
        );
        assert!(html.contains("p{margin:0 0 .7rem}"), "{html}");
        assert!(
            html.contains(
                "blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem;border-left:4px solid #134e4a;color:#134e4a}"
            ),
            "quote bar must not regress: {html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th teal must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_html_link_hover_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "letter.md must keep the DocAgent hyperlink"
        );
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let link = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) => p.runs.iter().find_map(|r| match &r.content {
                    RunContent::Inline(InlineObject::Hyperlink { target, display }) => {
                        Some((target.as_str(), display.as_str(), r.style.underline))
                    }
                    _ => None,
                }),
                _ => None,
            })
            .expect("letter hyperlink");
        assert_eq!(link.0, "https://github.com/kevin9327/docagent");
        assert_eq!(link.1, "DocAgent");
        assert_eq!(link.2, docagent_model::Underline::Single);
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let t = first_table(&doc);
        assert_eq!(
            t.rows[0].cells[0].shading,
            Some(TH_BG),
            "th must not regress"
        );
        assert!(
            doc.sections[0].body.iter().any(|b| match b {
                Block::Paragraph(p) => p.runs.iter().any(|r| {
                    r.style.code
                        && r.style.font.as_deref() == Some(CODE_FONT)
                        && matches!(&r.content, RunContent::Text(t) if t == "prove")
                }),
                _ => false,
            }),
            "letter.md `prove` must stay Consolas"
        );
        let html = to_html(&doc);
        assert!(
            html.contains(r#"<a href="https://github.com/kevin9327/docagent">DocAgent</a>"#),
            "{html}"
        );
        assert!(
            html.contains(LINK_RULE) && html.contains(LINK_HOVER_RULE),
            "letter.md HTML links must stay teal rest + #134e4a hover: {html}"
        );
        assert!(
            html.contains("a{color:#0d9488;text-decoration:underline}a:visited{color:#0d9488}a:hover{color:#134e4a}a:focus{color:#134e4a}"),
            "letter.md HTML hover must sit next to rest: {html}"
        );
        assert!(
            !html.contains("a:hover{color:blue")
                && !html.contains("a:hover{color:red")
                && !html.contains("a:hover{color:#111827"),
            "letter.md HTML must not keep UA hover: {html}"
        );
        assert!(
            html.contains(BODY_RULE)
                && html.contains("body{margin:0;background:#f8fafc;color:#134e4a}"),
            "letter.md HTML body type must stay #134e4a: {html}"
        );
        assert!(
            html.contains(PAGE_RULE),
            "@page A4 2.5rem/2.75rem must not regress: {html}"
        );
        assert!(
            html.contains(ARTICLE_RULE) && html.contains(r#"article{max-width:48rem"#),
            "48rem measure must not regress: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "Consolas must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"),
            "hr color must stay letter #134e4a: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(
            !html.contains("color:#111827"),
            "letter.md HTML must not keep slate #111827: {html}"
        );
    }

    #[test]
    fn article_css_link_visited_focus_and_landed_weasyprint() {
        let html = to_html(&Document::new());
        assert_eq!(LINK_RULE, "a{color:#0d9488;text-decoration:underline}");
        assert_eq!(LINK_VISITED_RULE, "a:visited{color:#0d9488}");
        assert_eq!(LINK_HOVER_RULE, "a:hover{color:#134e4a}");
        assert_eq!(LINK_FOCUS_RULE, "a:focus{color:#134e4a}");
        assert_ne!(
            LINK_VISITED_RULE, "a:visited{color:purple}",
            "UA purple visited is not WeasyPrint letter input"
        );
        assert_ne!(
            LINK_FOCUS_RULE, "a:focus{color:blue}",
            "UA blue focus is not WeasyPrint letter input"
        );
        assert_ne!(
            LINK_HOVER_RULE, "a:hover{color:blue}",
            "UA blue hover is not WeasyPrint letter input"
        );
        assert!(
            html.contains(LINK_RULE) && html.contains(LINK_VISITED_RULE),
            "visited links must stay letter teal #0d9488, not UA purple: {html}"
        );
        assert!(
            html.contains(LINK_HOVER_RULE) && html.contains(LINK_FOCUS_RULE),
            "focus must rest to letter teal #134e4a like hover, not UA ring-only: {html}"
        );
        assert!(
            html.contains("a{color:#0d9488;text-decoration:underline}a:visited{color:#0d9488}a:hover{color:#134e4a}a:focus{color:#134e4a}"),
            "LVHF order: visited before hover so visited:hover still rests: {html}"
        );
        assert!(
            !html.contains("a:visited{color:purple")
                && !html.contains("a:visited{color:#551a8b")
                && !html.contains("a:visited{color:#000")
                && !html.contains("a:visited{color:#111827")
                && !html.contains("a:visited{text-decoration:none")
                && !html.contains("a:focus{color:blue")
                && !html.contains("a:focus{color:red")
                && !html.contains("a:focus{color:#000")
                && !html.contains("a:focus{color:#111827")
                && !html.contains("a:hover{color:blue")
                && !html.contains("a:hover{color:red"),
            "UA visited/focus/hover colors are not WeasyPrint letter input: {html}"
        );
        assert_eq!(BODY_RULE, "body{margin:0;background:#f8fafc;color:#134e4a}");
        assert!(
            html.contains(BODY_RULE)
                && html.contains("body{margin:0;background:#f8fafc;color:#134e4a}"),
            "body color must stay letter teal #134e4a: {html}"
        );
        assert_eq!(PAGE_RULE, "@page{size:A4;margin:2.5rem 2.75rem}");
        assert!(
            html.contains(PAGE_RULE),
            "@page A4 2.5rem/2.75rem must not regress: {html}"
        );
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48);
        assert!(
            html.contains(ARTICLE_RULE) && html.contains(r#"article{max-width:48rem"#),
            "48rem measure must not regress: {html}"
        );
        assert!(
            html.contains("padding:2.5rem 2.75rem"),
            "article pad must not regress: {html}"
        );
        assert_eq!(CODE_FONT, "Consolas");
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "Consolas must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"),
            "hr color must stay letter #134e4a: {html}"
        );
        assert!(
            !html.contains("color:#111827"),
            "slate #111827 body type is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(PRINT_RULE),
            "print @page inset must not regress: {html}"
        );
        assert!(
            html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem"),
            "cell pad must not regress: {html}"
        );
        assert!(
            html.contains("h1{font-size:1.75rem;line-height:1.2"),
            "{html}"
        );
        assert!(
            html.contains("h2{font-size:1.15rem;line-height:1.2"),
            "{html}"
        );
        assert!(html.contains("p{margin:0 0 .7rem}"), "{html}");
        assert!(
            html.contains(
                "blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem;border-left:4px solid #134e4a;color:#134e4a}"
            ),
            "quote bar must not regress: {html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th teal must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_html_link_visited_focus_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "letter.md must keep the DocAgent hyperlink"
        );
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let link = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) => p.runs.iter().find_map(|r| match &r.content {
                    RunContent::Inline(InlineObject::Hyperlink { target, display }) => {
                        Some((target.as_str(), display.as_str(), r.style.underline))
                    }
                    _ => None,
                }),
                _ => None,
            })
            .expect("letter hyperlink");
        assert_eq!(link.0, "https://github.com/kevin9327/docagent");
        assert_eq!(link.1, "DocAgent");
        assert_eq!(link.2, docagent_model::Underline::Single);
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let t = first_table(&doc);
        assert_eq!(
            t.rows[0].cells[0].shading,
            Some(TH_BG),
            "th must not regress"
        );
        assert!(
            doc.sections[0].body.iter().any(|b| match b {
                Block::Paragraph(p) => p.runs.iter().any(|r| {
                    r.style.code
                        && r.style.font.as_deref() == Some(CODE_FONT)
                        && matches!(&r.content, RunContent::Text(t) if t == "prove")
                }),
                _ => false,
            }),
            "letter.md `prove` must stay Consolas"
        );
        let html = to_html(&doc);
        assert!(
            html.contains(r#"<a href="https://github.com/kevin9327/docagent">DocAgent</a>"#),
            "{html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "letter.md HTML links must stay teal rest + visited + #134e4a hover/focus: {html}"
        );
        assert!(
            html.contains("a{color:#0d9488;text-decoration:underline}a:visited{color:#0d9488}a:hover{color:#134e4a}a:focus{color:#134e4a}"),
            "letter.md HTML LVHF must sit in cascade order: {html}"
        );
        assert!(
            !html.contains("a:visited{color:purple")
                && !html.contains("a:visited{color:#551a8b")
                && !html.contains("a:focus{color:blue")
                && !html.contains("a:hover{color:blue")
                && !html.contains("a:hover{color:red")
                && !html.contains("a:hover{color:#111827"),
            "letter.md HTML must not keep UA visited/focus/hover: {html}"
        );
        assert!(
            html.contains(BODY_RULE)
                && html.contains("body{margin:0;background:#f8fafc;color:#134e4a}"),
            "letter.md HTML body type must stay #134e4a: {html}"
        );
        assert!(
            html.contains(PAGE_RULE),
            "@page A4 2.5rem/2.75rem must not regress: {html}"
        );
        assert!(
            html.contains(ARTICLE_RULE) && html.contains(r#"article{max-width:48rem"#),
            "48rem measure must not regress: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "Consolas must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"),
            "hr color must stay letter #134e4a: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(
            !html.contains("color:#111827"),
            "letter.md HTML must not keep slate #111827: {html}"
        );
    }

    #[test]
    fn article_css_thead_and_first_row_th() {
        let html = to_html(&Document::new());
        assert_eq!(THEAD_RULE, "thead{display:table-header-group}");
        assert_eq!(
            TH_RULE,
            "thead th,tr:first-child th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"
        );
        assert_ne!(
            TH_RULE,
            "th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}",
            "bare th{{}} paints body-row th like the hop header"
        );
        assert!(
            html.contains(THEAD_RULE) && html.contains(TH_RULE),
            "article CSS must pin thead repeat + first-row th fill, not a first-row-only band: {html}"
        );
        assert!(
            html.contains("table{border-collapse:collapse;margin:1rem 0;width:100%}thead{display:table-header-group}tbody{display:table-row-group}th,td{border:1px solid #cbd5e1;padding:.4rem .65rem;text-align:left;font-size:.95rem}thead th,tr:first-child th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "thead / tbody / first-row th must sit after table collapse in cascade order: {html}"
        );
        assert!(
            !html.contains("}th{background:#ccfbf1")
                && !html.contains(";th{background:#ccfbf1")
                && !html.contains("thead{display:table-row-group")
                && !html.contains("thead{display:none"),
            "bare th{{}} / hidden thead is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "body color / 48rem / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_html_thead_first_row_th_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("| Hop |"),
            "letter.md must keep the hop table"
        );
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let t = first_table(&doc);
        assert!(t.rows[0].header, "letter hop row must be a header");
        assert_eq!(t.header_row_count, 1);
        assert_eq!(
            t.rows[0].cells[0].shading,
            Some(TH_BG),
            "th must not regress"
        );
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let html = to_html(&doc);
        assert!(
            html.contains("<thead><tr><th><strong>Hop</strong></th><th><strong>File</strong></th><th><strong>Proof</strong></th></tr></thead>"),
            "letter.md hop header must emit <thead>, not a first-row <tr> WeasyPrint will not repeat: {html}"
        );
        assert!(
            html.contains("<tbody><tr><td>Input</td><td>letter.md</td><td>input SHA-256</td></tr>"),
            "letter.md hop body must sit in <tbody>: {html}"
        );
        assert!(
            !html.contains("<table><tr><th>") && !html.contains("</th></tr><tr><td>"),
            "letter.md HTML must not keep a thead-less first-row th band: {html}"
        );
        assert!(
            html.contains(THEAD_RULE) && html.contains(TBODY_RULE) && html.contains(TH_RULE),
            "letter.md HTML must pin thead repeat + tbody row-group + thead/first-row th fill: {html}"
        );
        assert!(
            html.contains("thead th,tr:first-child th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "letter.md HTML th fill must cover thead and first-row th, not bare th{{}}: {html}"
        );
        let back = read(html.as_bytes()).expect("html");
        let back_t = first_table(&back);
        assert!(back_t.rows[0].header);
        assert!(!back_t.rows[1].header);
        assert_eq!(back_t.header_row_count, 1);
        assert_eq!(back_t.rows[0].cells[0].shading, Some(TH_BG));
        assert_eq!(back_t.rows[1].cells[0].shading, None);
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE)
                && html.contains("body{margin:0;background:#f8fafc;color:#134e4a}"),
            "letter.md HTML body type must stay #134e4a: {html}"
        );
        assert!(
            html.contains(PAGE_RULE)
                && html.contains(ARTICLE_RULE)
                && html.contains(r#"article{max-width:48rem"#),
            "48rem / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(
            !html.contains("color:#111827"),
            "letter.md HTML must not keep slate #111827: {html}"
        );
    }

    #[test]
    fn article_css_print_color_adjust_exact() {
        let html = to_html(&Document::new());
        assert_eq!(
            PRINT_COLOR_RULE,
            "html{-webkit-print-color-adjust:exact;print-color-adjust:exact}"
        );
        assert_ne!(
            PRINT_COLOR_RULE, "html{print-color-adjust:economy}",
            "economy drops th #ccfbf1 / code chips / mark amber in print"
        );
        assert!(
            html.contains(PRINT_COLOR_RULE),
            "article CSS must pin print-color-adjust:exact so WeasyPrint keeps th/code/mark/pre/task fills: {html}"
        );
        assert!(
            html.contains("@media print{body{background:#fff}article{margin:0 auto;border:none;padding:0}}html{-webkit-print-color-adjust:exact;print-color-adjust:exact}"),
            "print-color-adjust must sit after @media print in cascade order: {html}"
        );
        assert!(
            html.contains("-webkit-print-color-adjust:exact")
                && html.contains("print-color-adjust:exact"),
            "WebKit + standard exact adjust must both be present: {html}"
        );
        assert!(
            !html.contains("print-color-adjust:economy")
                && !html.contains("-webkit-print-color-adjust:economy")
                && !html.contains("print-color-adjust:inherit")
                && !html.contains("print-color-adjust:unset"),
            "economy/inherit print-color-adjust drops letter fills: {html}"
        );
        assert!(
            html.contains(PRINT_RULE) && html.contains(PAGE_RULE),
            "print @page inset must not regress: {html}"
        );
        assert!(
            html.contains(THEAD_RULE) && html.contains(TH_RULE),
            "thead fill must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE),
            "body color / 48rem must not regress: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "code chip fill that print-color-adjust keeps must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark fill that print-color-adjust keeps must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_html_print_color_adjust_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let t = first_table(&doc);
        assert_eq!(
            t.rows[0].cells[0].shading,
            Some(TH_BG),
            "th fill must not regress"
        );
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let html = to_html(&doc);
        assert!(
            html.contains(PRINT_COLOR_RULE)
                && html.contains("html{-webkit-print-color-adjust:exact;print-color-adjust:exact}"),
            "letter.md HTML must pin print-color-adjust:exact so WeasyPrint keeps hop th / prove chip / mark: {html}"
        );
        assert!(
            html.contains("@media print{body{background:#fff}article{margin:0 auto;border:none;padding:0}}html{-webkit-print-color-adjust:exact;print-color-adjust:exact}"),
            "letter.md HTML print-color-adjust must sit after @media print: {html}"
        );
        assert!(
            !html.contains("print-color-adjust:economy")
                && !html.contains("-webkit-print-color-adjust:economy"),
            "letter.md HTML must not keep economy print-color-adjust: {html}"
        );
        assert!(
            html.contains(PRINT_RULE) && html.contains(PAGE_RULE),
            "@page A4 2.5rem/2.75rem must not regress: {html}"
        );
        assert!(
            html.contains(THEAD_RULE) && html.contains(TH_RULE),
            "thead / first-row th fill must not regress: {html}"
        );
        assert!(
            html.contains("th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "th #ccfbf1 that print-color-adjust keeps must not regress: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "prove chip that print-color-adjust keeps must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark amber that print-color-adjust keeps must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE)
                && html.contains(ARTICLE_RULE)
                && html.contains(r#"article{max-width:48rem"#),
            "48rem / body teal must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(html.contains("<mark>three hashes</mark>"), "{html}");
        assert!(
            !html.contains("color:#111827"),
            "letter.md HTML must not keep slate #111827: {html}"
        );
    }

    #[test]
    fn article_css_kbd_samp_match_code_chip() {
        let html = to_html(&Document::new());
        assert_eq!(
            KBD_SAMP_RULE,
            "kbd,samp{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"
        );
        assert_ne!(
            KBD_SAMP_RULE, "kbd,samp{font-family:monospace}",
            "WeasyPrint UA kbd/samp is bare monospace, not the letter.html prove chip"
        );
        assert!(
            html.contains(KBD_SAMP_RULE),
            "article CSS must pin kbd/samp to the code chip so WeasyPrint does not paint a 1px-border key: {html}"
        );
        assert!(
            html.contains("thead th,tr:first-child th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}kbd,samp{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "kbd/samp chip must sit after thead th in cascade order: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "code chip that kbd/samp matches must not regress: {html}"
        );
        assert!(
            !html.contains("kbd{border:1px")
                && !html.contains("kbd{font-family:serif")
                && !html.contains("samp{font-family:serif")
                && !html.contains("kbd,samp{font-family:monospace}")
                && !html.contains("kbd,samp{background:none")
                && !html.contains("kbd,samp{padding:0"),
            "UA kbd/samp is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(PRINT_COLOR_RULE) && html.contains(TH_RULE) && html.contains(THEAD_RULE),
            "print-color-adjust / thead fill must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "body color / 48rem / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_html_kbd_samp_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        assert_eq!(
            first_table(&doc).rows[0].cells[0].shading,
            Some(TH_BG),
            "th fill must not regress"
        );
        let html = to_html(&doc);
        assert!(
            html.contains(KBD_SAMP_RULE)
                && html.contains("kbd,samp{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "letter.md HTML must pin kbd/samp to the prove chip: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "prove chip that kbd/samp matches must not regress: {html}"
        );
        assert!(html.contains("<code>prove</code>"), "{html}");
        let keyed =
            read(b"<p>Run <kbd>prove</kbd> twice. Output <samp>SHA-256</samp>.</p>").unwrap();
        let p = first_para(&keyed);
        assert!(
            p.runs.iter().any(|r| {
                r.style.code
                    && r.style.font.as_deref() == Some(CODE_FONT)
                    && matches!(&r.content, RunContent::Text(t) if t == "prove")
            }),
            "<kbd>prove</kbd> must read as the Consolas code chip: {:?}",
            p.runs
                .iter()
                .map(|r| (
                    r.style.code,
                    r.style.font.clone(),
                    r.display_text().to_string()
                ))
                .collect::<Vec<_>>()
        );
        assert!(
            p.runs.iter().any(|r| {
                r.style.code
                    && r.style.font.as_deref() == Some(CODE_FONT)
                    && matches!(&r.content, RunContent::Text(t) if t == "SHA-256")
            }),
            "<samp>SHA-256</samp> must read as the Consolas code chip: {:?}",
            p.runs
                .iter()
                .map(|r| (
                    r.style.code,
                    r.style.font.clone(),
                    r.display_text().to_string()
                ))
                .collect::<Vec<_>>()
        );
        assert!(
            html.contains(PRINT_COLOR_RULE) && html.contains(TH_RULE) && html.contains(THEAD_RULE),
            "print-color-adjust / thead fill must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "48rem / body teal / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(
            !html.contains("color:#111827"),
            "letter.md HTML must not keep slate #111827: {html}"
        );
    }

    #[test]
    fn article_css_figure_figcaption_tight() {
        let html = to_html(&Document::new());
        assert_eq!(
            FIGURE_RULE,
            "figure{margin:.6rem 0 1rem}figure img{margin:0}figcaption{font-size:.85rem;line-height:1.45;color:#134e4a;margin:.35rem 0 0;text-align:left;font-style:normal}"
        );
        assert_ne!(
            FIGURE_RULE, "figure{margin:1em 40px}",
            "WeasyPrint UA figure 40px side margin punches holes in the 48rem column"
        );
        assert!(
            html.contains(FIGURE_RULE),
            "article CSS must pin figure/figcaption so WeasyPrint does not paint UA 40px + italic center: {html}"
        );
        assert!(
            html.contains("kbd,samp{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}figure{margin:.6rem 0 1rem}figure img{margin:0}figcaption{font-size:.85rem;line-height:1.45;color:#134e4a;margin:.35rem 0 0;text-align:left;font-style:normal}"),
            "figure/figcaption must sit after kbd/samp in cascade order: {html}"
        );
        assert!(
            html.contains("figure{margin:.6rem 0 1rem}")
                && html.contains("figure img{margin:0}")
                && html.contains("figcaption{font-size:.85rem;line-height:1.45;color:#134e4a;margin:.35rem 0 0;text-align:left;font-style:normal}"),
            "figure must kill UA 40px sides; figcaption must stay teal left, not italic center: {html}"
        );
        assert!(
            !html.contains("figure{margin:1em 40px")
                && !html.contains("figure{margin:40px")
                && !html.contains("figcaption{text-align:center")
                && !html.contains("figcaption{font-style:italic")
                && !html.contains("figcaption{color:#64748b")
                && !html.contains("figure{margin:1em 40px}"),
            "UA figure 40px / italic centered figcaption is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(KBD_SAMP_RULE)
                && html.contains(PRINT_COLOR_RULE)
                && html.contains(TH_RULE),
            "kbd/samp / print-color-adjust / thead fill must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "body color / 48rem / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_html_figure_figcaption_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        assert_eq!(
            first_table(&doc).rows[0].cells[0].shading,
            Some(TH_BG),
            "th fill must not regress"
        );
        let html = to_html(&doc);
        assert!(
            html.contains(FIGURE_RULE)
                && html.contains("figure{margin:.6rem 0 1rem}figure img{margin:0}figcaption{font-size:.85rem;line-height:1.45;color:#134e4a;margin:.35rem 0 0;text-align:left;font-style:normal}"),
            "letter.md HTML must pin figure/figcaption tightness: {html}"
        );
        assert!(
            html.contains(KBD_SAMP_RULE)
                && html.contains("kbd,samp{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "kbd/samp chip must not regress: {html}"
        );
        let figured = read(
            br#"<figure><img alt="Harbor mark" src="docs/assets/mark.png"/><figcaption>Harbor mark</figcaption></figure>"#,
        )
        .expect("figure html");
        let figured_img = first_image(&figured);
        assert_eq!(figured_img.alt_text.as_deref(), Some("Harbor mark"));
        assert!(
            figured_img.width >= MIN_IMAGE_EDGE_HU && figured_img.height >= MIN_IMAGE_EDGE_HU,
            "figure img must keep the 24pt floor"
        );
        assert!(
            figured.sections[0].body.iter().any(|b| match b {
                Block::Paragraph(p) =>
                    p.plain_text().contains("Harbor mark") && p.outline_level.is_none(),
                _ => false,
            }),
            "figcaption text must round-trip: {:?}",
            figured.sections[0]
                .body
                .iter()
                .map(|b| match b {
                    Block::Paragraph(p) => p.plain_text(),
                    _ => String::new(),
                })
                .collect::<Vec<_>>()
        );
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(
            html.contains(PRINT_COLOR_RULE) && html.contains(TH_RULE) && html.contains(THEAD_RULE),
            "print-color-adjust / thead fill must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "48rem / body teal / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(
            !html.contains("color:#111827"),
            "letter.md HTML must not keep slate #111827: {html}"
        );
    }

    #[test]
    fn article_css_caption_side_tight() {
        let html = to_html(&Document::new());
        assert_eq!(
            CAPTION_RULE,
            "caption{caption-side:bottom;text-align:left;font-size:.85rem;line-height:1.45;color:#134e4a;padding:.35rem 0 0;font-style:normal}"
        );
        assert_ne!(
            CAPTION_RULE, "caption{caption-side:top;text-align:center}",
            "WeasyPrint UA caption is a centered band above the table, not letter figcaption"
        );
        assert!(
            html.contains(CAPTION_RULE),
            "article CSS must pin caption-side so WeasyPrint does not paint UA top+center: {html}"
        );
        assert!(
            html.contains("figcaption{font-size:.85rem;line-height:1.45;color:#134e4a;margin:.35rem 0 0;text-align:left;font-style:normal}caption{caption-side:bottom;text-align:left;font-size:.85rem;line-height:1.45;color:#134e4a;padding:.35rem 0 0;font-style:normal}"),
            "caption must sit after figcaption in cascade order: {html}"
        );
        assert!(
            html.contains("caption{caption-side:bottom")
                && html.contains("caption{caption-side:bottom;text-align:left")
                && html.contains("caption-side:bottom")
                && html.contains(FIGURE_RULE),
            "caption-side must stay bottom/left/teal like figcaption, not UA top/center: {html}"
        );
        assert!(
            !html.contains("caption{caption-side:top")
                && !html.contains("caption{text-align:center")
                && !html.contains("caption{font-style:italic")
                && !html.contains("caption{color:#64748b")
                && !html.contains("caption{caption-side:top;text-align:center}"),
            "UA top/center/italic caption is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(FIGURE_RULE)
                && html.contains(KBD_SAMP_RULE)
                && html.contains(PRINT_COLOR_RULE)
                && html.contains(TH_RULE),
            "figure / kbd/samp / print-color-adjust / thead fill must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "body color / 48rem / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("table{border-collapse:collapse;margin:1rem 0;width:100%}"),
            "table collapse that caption-side sits on must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_html_caption_side_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        assert_eq!(
            first_table(&doc).rows[0].cells[0].shading,
            Some(TH_BG),
            "th fill must not regress"
        );
        let html = to_html(&doc);
        assert!(
            html.contains(CAPTION_RULE)
                && html.contains("caption{caption-side:bottom;text-align:left;font-size:.85rem;line-height:1.45;color:#134e4a;padding:.35rem 0 0;font-style:normal}"),
            "letter.md HTML must pin caption-side tightness: {html}"
        );
        assert!(
            html.contains(FIGURE_RULE)
                && html.contains("figure{margin:.6rem 0 1rem}figure img{margin:0}figcaption{font-size:.85rem;line-height:1.45;color:#134e4a;margin:.35rem 0 0;text-align:left;font-style:normal}"),
            "figure/figcaption must not regress: {html}"
        );
        let captioned = read(
            br#"<table><caption>Hop table</caption><thead><tr><th>Hop</th></tr></thead><tbody><tr><td>Input</td></tr></tbody></table>"#,
        )
        .expect("caption html");
        let cap_t = first_table(&captioned);
        assert!(cap_t.rows[0].header, "thead row must stay a header");
        assert!(!cap_t.rows[1].header, "tbody row must stay body");
        assert_eq!(cap_t.header_row_count, 1);
        assert_eq!(cap_t.rows[0].cells[0].shading, Some(TH_BG));
        let cap_out = to_html(&captioned);
        assert!(
            cap_out.contains(CAPTION_RULE)
                && cap_out.contains("caption-side:bottom")
                && !cap_out.contains("caption{caption-side:top"),
            "round-trip HTML must keep letter caption-side, not UA top: {cap_out}"
        );
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(
            html.contains(PRINT_COLOR_RULE) && html.contains(TH_RULE) && html.contains(THEAD_RULE),
            "print-color-adjust / thead fill must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "48rem / body teal / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(
            html.contains("table{border-collapse:collapse;margin:1rem 0;width:100%}"),
            "table collapse must not regress: {html}"
        );
        assert!(
            !html.contains("color:#111827"),
            "letter.md HTML must not keep slate #111827: {html}"
        );
    }

    #[test]
    fn article_css_selection_color_tight() {
        let html = to_html(&Document::new());
        assert_eq!(
            SELECTION_RULE,
            "::selection{background:#ccfbf1;color:#134e4a}::-moz-selection{background:#ccfbf1;color:#134e4a}"
        );
        assert_ne!(
            SELECTION_RULE, "::selection{background:Highlight;color:HighlightText}",
            "UA Highlight/HighlightText is system blue, not the letter.html mint chip"
        );
        assert!(
            html.contains(SELECTION_RULE),
            "article CSS must pin ::selection so UA system-blue does not punch the teal chip: {html}"
        );
        assert!(
            html.contains("caption{caption-side:bottom;text-align:left;font-size:.85rem;line-height:1.45;color:#134e4a;padding:.35rem 0 0;font-style:normal}::selection{background:#ccfbf1;color:#134e4a}::-moz-selection{background:#ccfbf1;color:#134e4a}"),
            "::selection must sit after caption in cascade order: {html}"
        );
        assert!(
            html.contains("::selection{background:#ccfbf1;color:#134e4a}")
                && html.contains("::-moz-selection{background:#ccfbf1;color:#134e4a}")
                && html.contains(CAPTION_RULE),
            "selection fill must stay mint chip + body teal, not UA Highlight: {html}"
        );
        assert!(
            !html.contains("::selection{background:Highlight")
                && !html.contains("::selection{background:blue")
                && !html.contains("::selection{background:#3390ff")
                && !html.contains("::selection{background:#0078d7")
                && !html.contains("::selection{color:#fff")
                && !html.contains("::selection{background:Highlight;color:HighlightText}"),
            "UA system-blue / Highlight selection is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(CAPTION_RULE)
                && html.contains(FIGURE_RULE)
                && html.contains(KBD_SAMP_RULE)
                && html.contains(PRINT_COLOR_RULE)
                && html.contains(TH_RULE),
            "caption / figure / kbd/samp / print-color-adjust / thead fill must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "body color / 48rem / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("table{border-collapse:collapse;margin:1rem 0;width:100%}"),
            "table collapse must not regress: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "code chip that selection matches must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_html_selection_color_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        assert_eq!(
            first_table(&doc).rows[0].cells[0].shading,
            Some(TH_BG),
            "th fill must not regress"
        );
        let html = to_html(&doc);
        assert!(
            html.contains(SELECTION_RULE)
                && html.contains("::selection{background:#ccfbf1;color:#134e4a}::-moz-selection{background:#ccfbf1;color:#134e4a}"),
            "letter.md HTML must pin ::selection tightness: {html}"
        );
        assert!(
            html.contains(CAPTION_RULE)
                && html.contains("caption{caption-side:bottom;text-align:left;font-size:.85rem;line-height:1.45;color:#134e4a;padding:.35rem 0 0;font-style:normal}"),
            "caption-side must not regress: {html}"
        );
        assert!(
            html.contains(FIGURE_RULE)
                && html.contains("figure{margin:.6rem 0 1rem}figure img{margin:0}figcaption{font-size:.85rem;line-height:1.45;color:#134e4a;margin:.35rem 0 0;text-align:left;font-style:normal}"),
            "figure/figcaption must not regress: {html}"
        );
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(
            html.contains(PRINT_COLOR_RULE) && html.contains(TH_RULE) && html.contains(THEAD_RULE),
            "print-color-adjust / thead fill must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "48rem / body teal / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(
            html.contains("table{border-collapse:collapse;margin:1rem 0;width:100%}"),
            "table collapse must not regress: {html}"
        );
        assert!(
            !html.contains("::selection{background:Highlight")
                && !html.contains("::selection{background:blue")
                && !html.contains("color:#111827"),
            "letter.md HTML must not keep UA Highlight selection or slate #111827: {html}"
        );
    }

    #[test]
    fn article_css_abbr_dotted_teal_tight() {
        let html = to_html(&Document::new());
        assert_eq!(
            ABBR_RULE,
            "abbr[title],acronym[title]{text-decoration:underline dotted #134e4a;text-underline-offset:.15em;cursor:help}"
        );
        assert_ne!(
            ABBR_RULE, "abbr[title],acronym[title]{text-decoration:dotted underline}",
            "WeasyPrint UA abbr dotted underline is uncolored CanvasText, not letter teal"
        );
        assert!(
            html.contains(ABBR_RULE),
            "article CSS must pin abbr dotted underline to body teal so UA black dots do not punch the chip: {html}"
        );
        assert!(
            html.contains("::selection{background:#ccfbf1;color:#134e4a}::-moz-selection{background:#ccfbf1;color:#134e4a}abbr[title],acronym[title]{text-decoration:underline dotted #134e4a;text-underline-offset:.15em;cursor:help}"),
            "abbr must sit after ::selection in cascade order: {html}"
        );
        assert!(
            html.contains("abbr[title],acronym[title]{text-decoration:underline dotted #134e4a")
                && html.contains("text-underline-offset:.15em")
                && html.contains("cursor:help")
                && html.contains(SELECTION_RULE),
            "abbr title underline must stay dotted body teal, not UA uncolored: {html}"
        );
        assert!(
            !html.contains("abbr[title],acronym[title]{text-decoration:dotted underline}")
                && !html.contains("abbr{text-decoration:underline}")
                && !html.contains("abbr[title]{text-decoration:underline}")
                && !html.contains("abbr[title]{border-bottom:1px dotted")
                && !html.contains("abbr[title]{text-decoration:dotted underline;color:#000"),
            "UA uncolored / black dotted abbr is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(SELECTION_RULE)
                && html.contains(CAPTION_RULE)
                && html.contains(FIGURE_RULE)
                && html.contains(KBD_SAMP_RULE)
                && html.contains(PRINT_COLOR_RULE)
                && html.contains(TH_RULE),
            "selection / caption / figure / kbd/samp / print-color-adjust / thead fill must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "body color / 48rem / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("table{border-collapse:collapse;margin:1rem 0;width:100%}"),
            "table collapse must not regress: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "code chip that abbr teal matches must not regress: {html}"
        );
        assert!(
            html.contains("s,del{text-decoration:line-through;color:#134e4a}"),
            "s/del teal decoration that abbr dotted matches must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_html_abbr_dotted_teal_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        assert_eq!(
            first_table(&doc).rows[0].cells[0].shading,
            Some(TH_BG),
            "th fill must not regress"
        );
        let html = to_html(&doc);
        assert!(
            html.contains(ABBR_RULE)
                && html.contains("abbr[title],acronym[title]{text-decoration:underline dotted #134e4a;text-underline-offset:.15em;cursor:help}"),
            "letter.md HTML must pin abbr dotted-teal tightness: {html}"
        );
        assert!(
            html.contains(SELECTION_RULE)
                && html.contains("::selection{background:#ccfbf1;color:#134e4a}::-moz-selection{background:#ccfbf1;color:#134e4a}"),
            "::selection must not regress: {html}"
        );
        assert!(
            html.contains(CAPTION_RULE)
                && html.contains("caption{caption-side:bottom;text-align:left;font-size:.85rem;line-height:1.45;color:#134e4a;padding:.35rem 0 0;font-style:normal}"),
            "caption-side must not regress: {html}"
        );
        assert!(
            html.contains(FIGURE_RULE)
                && html.contains("figure{margin:.6rem 0 1rem}figure img{margin:0}figcaption{font-size:.85rem;line-height:1.45;color:#134e4a;margin:.35rem 0 0;text-align:left;font-style:normal}"),
            "figure/figcaption must not regress: {html}"
        );
        let abbreviated = read(
            br#"<p>Replay <abbr title="Portable Document Format / A">PDF/A</abbr> hashes.</p>"#,
        )
        .expect("abbr html");
        let abbr_out = to_html(&abbreviated);
        assert!(
            abbr_out.contains(ABBR_RULE)
                && abbr_out.contains("underline dotted #134e4a")
                && !abbr_out.contains("abbr[title],acronym[title]{text-decoration:dotted underline}"),
            "round-trip HTML must keep letter abbr teal, not UA uncolored dots: {abbr_out}"
        );
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(
            html.contains(PRINT_COLOR_RULE) && html.contains(TH_RULE) && html.contains(THEAD_RULE),
            "print-color-adjust / thead fill must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "48rem / body teal / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(
            html.contains("table{border-collapse:collapse;margin:1rem 0;width:100%}"),
            "table collapse must not regress: {html}"
        );
        assert!(
            !html.contains("abbr[title],acronym[title]{text-decoration:dotted underline}")
                && !html.contains("color:#111827"),
            "letter.md HTML must not keep UA uncolored abbr or slate #111827: {html}"
        );
    }

    #[test]
    fn article_css_heading_rule_tight() {
        let html = to_html(&Document::new());
        assert_eq!(
            HEADING_RULE,
            "h1{font-size:1.75rem;line-height:1.2;margin:0 0 .6rem;letter-spacing:-.02em;padding-bottom:.4rem;border-bottom:3px solid #134e4a}h2{font-size:1.15rem;line-height:1.2;margin:1.1rem 0 .45rem;color:#134e4a;padding-bottom:.2rem;border-bottom:2px solid #0d9488}"
        );
        assert_ne!(
            HEADING_RULE, "h1{font-size:2em;margin:.67em 0}h2{font-size:1.5em;margin:.75em 0}",
            "WeasyPrint UA h1/h2 has no teal bottom rule under the title"
        );
        assert!(
            html.contains(HEADING_RULE),
            "article CSS must pin h1/h2 bottom rules so WeasyPrint does not paint UA unruled titles: {html}"
        );
        let mut cascade = String::from(ARTICLE_RULE);
        cascade.push_str(HEADING_RULE);
        assert!(
            html.contains(&cascade),
            "h1/h2 rules must sit after article 48rem in cascade order: {html}"
        );
        assert!(
            html.contains("padding-bottom:.4rem;border-bottom:3px solid #134e4a")
                && html.contains("padding-bottom:.2rem;border-bottom:2px solid #0d9488")
                && html.contains("letter-spacing:-.02em")
                && html.contains(ARTICLE_RULE),
            "h1 must keep 3px #134e4a rule + .4rem pad; h2 must keep 2px #0d9488 rule + .2rem pad: {html}"
        );
        assert!(
            !html.contains("h1{font-size:2em")
                && !html.contains("h1{margin:.67em")
                && !html.contains("h1{border-bottom:none")
                && !html.contains("h1{border-bottom:1px")
                && !html.contains("h2{font-size:1.5em")
                && !html.contains("h2{border-bottom:none")
                && !html.contains("h1{border-bottom:1px solid #ccc")
                && !html.contains("h1{font-size:2em;margin:.67em 0}"),
            "UA unruled / 2em / 1px-gray heading is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(ABBR_RULE)
                && html.contains(SELECTION_RULE)
                && html.contains(CAPTION_RULE)
                && html.contains(FIGURE_RULE)
                && html.contains(KBD_SAMP_RULE)
                && html.contains(PRINT_COLOR_RULE)
                && html.contains(TH_RULE),
            "abbr / selection / caption / figure / kbd/samp / print-color-adjust / thead fill must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "body color / 48rem / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("table{border-collapse:collapse;margin:1rem 0;width:100%}"),
            "table collapse must not regress: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "code chip must not regress: {html}"
        );
        assert!(
            html.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"),
            "thematic hr that heading rules sit above must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_html_heading_rule_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let h1 = match &doc.sections[0].body[0] {
            Block::Paragraph(p) => p,
            _ => panic!("h1"),
        };
        let h2 = match &doc.sections[0].body[1] {
            Block::Paragraph(p) => p,
            _ => panic!("h2"),
        };
        assert_eq!(h1.outline_level, Some(1));
        assert_eq!(h1.runs[0].style.size, H1_SIZE_HU);
        assert_eq!(h1.line_spacing, LineSpacing::Percent(HEADING_LEADING_PCT));
        assert_eq!(h2.outline_level, Some(2));
        assert_eq!(h2.runs[0].style.size, H2_SIZE_HU);
        assert_eq!(h2.line_spacing, LineSpacing::Percent(HEADING_LEADING_PCT));
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        assert_eq!(
            first_table(&doc).rows[0].cells[0].shading,
            Some(TH_BG),
            "th fill must not regress"
        );
        let html = to_html(&doc);
        assert!(
            html.contains(HEADING_RULE)
                && html.contains("h1{font-size:1.75rem;line-height:1.2;margin:0 0 .6rem;letter-spacing:-.02em;padding-bottom:.4rem;border-bottom:3px solid #134e4a}")
                && html.contains("h2{font-size:1.15rem;line-height:1.2;margin:1.1rem 0 .45rem;color:#134e4a;padding-bottom:.2rem;border-bottom:2px solid #0d9488}"),
            "letter.md HTML must pin h1/h2 teal bottom rules: {html}"
        );
        assert!(
            html.contains("<h1>") && html.contains("Northwind Freight") && html.contains("</h1>"),
            "letter.md must keep the H1 title under the 3px rule: {html}"
        );
        assert!(
            html.contains("<h2>") && html.contains("Delivery confirmation") && html.contains("</h2>"),
            "letter.md must keep the H2 under the 2px #0d9488 rule: {html}"
        );
        let headed = read(
            b"<article><h1>Northwind Freight</h1><h2>Delivery confirmation</h2><p>The shipment left Busan.</p></article>",
        )
        .expect("heading html");
        let headed_out = to_html(&headed);
        assert!(
            headed_out.contains(HEADING_RULE)
                && headed_out.contains("padding-bottom:.4rem;border-bottom:3px solid #134e4a")
                && !headed_out.contains("h1{font-size:2em")
                && !headed_out.contains("h1{border-bottom:none"),
            "round-trip HTML must keep letter heading rules, not UA unruled 2em: {headed_out}"
        );
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(
            html.contains(ABBR_RULE) && html.contains(SELECTION_RULE) && html.contains(CAPTION_RULE),
            "abbr / selection / caption-side must not regress: {html}"
        );
        assert!(
            html.contains(FIGURE_RULE) && html.contains(KBD_SAMP_RULE),
            "figure / kbd/samp must not regress: {html}"
        );
        assert!(
            html.contains(PRINT_COLOR_RULE) && html.contains(TH_RULE) && html.contains(THEAD_RULE),
            "print-color-adjust / thead fill must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "48rem / body teal / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(
            html.contains("table{border-collapse:collapse;margin:1rem 0;width:100%}"),
            "table collapse must not regress: {html}"
        );
        assert!(
            !html.contains("h1{font-size:2em")
                && !html.contains("h1{border-bottom:none")
                && !html.contains("color:#111827"),
            "letter.md HTML must not keep UA unruled headings or slate #111827: {html}"
        );
    }

    #[test]
    fn article_css_mark_color_teal_tight() {
        let html = to_html(&Document::new());
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "article CSS must pin mark text to body teal so WeasyPrint UA black does not punch the chip: {html}"
        );
        assert!(
            html.contains("s,del{text-decoration:line-through;color:#134e4a}mark{background:#fde68a;padding:.05em .2em;color:#134e4a}code{background:#ccfbf1"),
            "mark teal must sit after s/del and before the code chip in cascade order: {html}"
        );
        assert!(
            !html.contains("mark{background:yellow;color:black")
                && !html.contains("mark{color:black")
                && !html.contains("mark{color:#000")
                && !html.contains("mark{background:yellow"),
            "UA yellow/black mark is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(ABBR_RULE)
                && html.contains(SELECTION_RULE)
                && html.contains(CAPTION_RULE)
                && html.contains(FIGURE_RULE)
                && html.contains(KBD_SAMP_RULE)
                && html.contains(PRINT_COLOR_RULE)
                && html.contains(TH_RULE)
                && html.contains(HEADING_RULE),
            "abbr / selection / caption / figure / kbd/samp / print-color-adjust / thead / heading must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "body color / 48rem / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_html_mark_color_and_img_path_keep_24pt() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("![Harbor mark](docs/assets/mark.png)"),
            "letter.md must keep the relative harbor-mark path"
        );
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let png = fixture_png();
        let img = first_image(&doc);
        assert_eq!(img.bytes, png);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let written = String::from_utf8(docagent_md::write(&doc).expect("md write")).unwrap();
        assert!(
            written.contains("![Harbor mark](docs/assets/mark.png)"),
            "markdown image path must round-trip: {written}"
        );
        assert!(
            !written.contains("data:image/png;base64,"),
            "letter.md path round-trip must not emit a data URI: {written}"
        );
        let again = docagent_md::read(written.as_bytes()).expect("md round-trip");
        assert_eq!(first_image(&again).bytes, png);
        let html = to_html(&doc);
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "letter.md HTML mark must stay amber + body teal, not UA black: {html}"
        );
        assert!(html.contains("<mark>three hashes</mark>"), "{html}");
        assert!(
            !html.contains("mark{background:yellow;color:black")
                && !html.contains("mark{color:black"),
            "letter.md HTML must not keep UA yellow/black mark: {html}"
        );
        let tag = first_img_tag(&html, "Harbor mark");
        assert!(
            tag.contains(r#"style="width:24pt;height:24pt""#),
            "WeasyPrint display size must sit on the <img>: {tag}"
        );
        assert_eq!(
            img_tag_display_pt(&html, "Harbor mark"),
            Some((MIN_IMAGE_EDGE_PT, MIN_IMAGE_EDGE_PT)),
            "letter.md <img> display size must match the PDF 24pt floor: {html}"
        );
        assert!(
            html.contains(ABBR_RULE)
                && html.contains(SELECTION_RULE)
                && html.contains(CAPTION_RULE)
                && html.contains(FIGURE_RULE)
                && html.contains(KBD_SAMP_RULE)
                && html.contains(PRINT_COLOR_RULE)
                && html.contains(THEAD_RULE)
                && html.contains(HEADING_RULE),
            "tightness lock must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "48rem / body teal / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains("<code>prove</code>"), "{html}");
        let rel = read(br#"<p><img alt="Harbor mark" src="docs/assets/mark.png"/></p>"#)
            .expect("relative html");
        let rel_html = to_html(&rel);
        assert_eq!(
            img_tag_display_pt(&rel_html, "Harbor mark"),
            Some((MIN_IMAGE_EDGE_PT, MIN_IMAGE_EDGE_PT)),
            "relative HTML src must still floor to 24pt: {rel_html}"
        );
        assert_eq!(first_image(&rel).bytes, png);
    }

    #[test]
    fn article_css_details_summary_tight() {
        let html = to_html(&Document::new());
        assert_eq!(
            DETAILS_RULE,
            "details{margin:0 0 .7rem;color:#134e4a}summary{display:block;cursor:pointer;font-weight:700;color:#134e4a;list-style:none}details[open]>summary{list-style:none}"
        );
        assert_ne!(
            DETAILS_RULE, "summary{display:list-item}",
            "HTML UA summary list-item paints a disclosure ▸, not letter block teal"
        );
        assert!(
            html.contains(DETAILS_RULE),
            "article CSS must pin details/summary so WeasyPrint/HTML UA ▸ does not punch the 48rem column: {html}"
        );
        assert!(
            html.contains("abbr[title],acronym[title]{text-decoration:underline dotted #134e4a;text-underline-offset:.15em;cursor:help}details{margin:0 0 .7rem;color:#134e4a}summary{display:block;cursor:pointer;font-weight:700;color:#134e4a;list-style:none}details[open]>summary{list-style:none}"),
            "details/summary must sit after abbr in cascade order: {html}"
        );
        assert!(
            html.contains("details{margin:0 0 .7rem;color:#134e4a}")
                && html.contains("summary{display:block;cursor:pointer;font-weight:700;color:#134e4a;list-style:none}")
                && html.contains("details[open]>summary{list-style:none}")
                && html.contains(ABBR_RULE),
            "details must keep p-like 0.7rem / body teal; summary must stay block + no ▸: {html}"
        );
        assert!(
            !html.contains("summary{display:list-item")
                && !html.contains("summary{list-style-type:disclosure-closed")
                && !html.contains("summary{list-style:disclosure-closed")
                && !html.contains("details{margin:1em 40px")
                && !html.contains("details{margin:40px"),
            "UA list-item / 40px details is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(ABBR_RULE)
                && html.contains(SELECTION_RULE)
                && html.contains(CAPTION_RULE)
                && html.contains(FIGURE_RULE)
                && html.contains(KBD_SAMP_RULE)
                && html.contains(PRINT_COLOR_RULE)
                && html.contains(TH_RULE)
                && html.contains(HEADING_RULE),
            "abbr / selection / caption / figure / kbd/samp / print-color-adjust / thead / heading must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "body color / 48rem / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("table{border-collapse:collapse;margin:1rem 0;width:100%}"),
            "table collapse must not regress: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "code chip must not regress: {html}"
        );
        assert!(
            html.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "task list-style:none that summary matches must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_html_details_summary_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        assert_eq!(
            first_table(&doc).rows[0].cells[0].shading,
            Some(TH_BG),
            "th fill must not regress"
        );
        let html = to_html(&doc);
        assert!(
            html.contains(DETAILS_RULE)
                && html.contains("details{margin:0 0 .7rem;color:#134e4a}")
                && html.contains("summary{display:block;cursor:pointer;font-weight:700;color:#134e4a;list-style:none}")
                && html.contains("details[open]>summary{list-style:none}"),
            "letter.md HTML must pin details/summary tightness: {html}"
        );
        let disclosed = read(
            b"<article><details><summary>Replay is evidence.</summary><p>Same fonts, same bytes.</p></details></article>",
        )
        .expect("details html");
        let disclosed_out = to_html(&disclosed);
        assert!(
            disclosed_out.contains(DETAILS_RULE)
                && disclosed_out.contains("summary{display:block")
                && !disclosed_out.contains("summary{display:list-item")
                && !disclosed_out.contains("details{margin:1em 40px"),
            "round-trip HTML must keep letter details/summary, not UA list-item ▸: {disclosed_out}"
        );
        assert!(
            disclosed.plain_text().contains("Replay is evidence")
                && disclosed.plain_text().contains("Same fonts, same bytes"),
            "details/summary inner text must still parse into IR: {}",
            disclosed.plain_text()
        );
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(
            html.contains(ABBR_RULE) && html.contains(SELECTION_RULE) && html.contains(CAPTION_RULE),
            "abbr / selection / caption-side must not regress: {html}"
        );
        assert!(
            html.contains(FIGURE_RULE) && html.contains(KBD_SAMP_RULE),
            "figure / kbd/samp must not regress: {html}"
        );
        assert!(
            html.contains(PRINT_COLOR_RULE) && html.contains(TH_RULE) && html.contains(THEAD_RULE),
            "print-color-adjust / thead fill must not regress: {html}"
        );
        assert!(
            html.contains(HEADING_RULE),
            "heading rules must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "48rem / body teal / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(
            html.contains("table{border-collapse:collapse;margin:1rem 0;width:100%}"),
            "table collapse must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
        assert!(
            !html.contains("summary{display:list-item")
                && !html.contains("h1{font-size:2em")
                && !html.contains("color:#111827"),
            "letter.md HTML must not keep UA disclosure summary, unruled headings, or slate #111827: {html}"
        );
    }

    #[test]
    fn article_css_tbody_row_group_tight() {
        let html = to_html(&Document::new());
        assert_eq!(TBODY_RULE, "tbody{display:table-row-group}");
        assert_ne!(
            TBODY_RULE, "tbody{display:block}",
            "display:block tbody stacks hop body rows, not WeasyPrint letter row-group"
        );
        assert!(
            html.contains(TBODY_RULE),
            "article CSS must pin tbody row-group so a display:block table reset does not punch the 48rem column: {html}"
        );
        assert!(
            html.contains("thead{display:table-header-group}tbody{display:table-row-group}th,td{border:1px solid #cbd5e1;padding:.4rem .65rem;text-align:left;font-size:.95rem}"),
            "tbody row-group must sit after thead and before th,td in cascade order: {html}"
        );
        assert!(
            html.contains("tbody{display:table-row-group}")
                && html.contains(THEAD_RULE)
                && html.contains("table{border-collapse:collapse;margin:1rem 0;width:100%}"),
            "tbody must keep table-row-group next to thead + collapse: {html}"
        );
        assert!(
            !html.contains("tbody{display:block")
                && !html.contains("tbody{display:table-header-group")
                && !html.contains("tbody{display:none")
                && !html.contains("tbody{display:table-footer-group"),
            "block / header-group / hidden tbody is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(DETAILS_RULE)
                && html.contains(ABBR_RULE)
                && html.contains(SELECTION_RULE)
                && html.contains(CAPTION_RULE)
                && html.contains(FIGURE_RULE)
                && html.contains(KBD_SAMP_RULE)
                && html.contains(PRINT_COLOR_RULE)
                && html.contains(TH_RULE)
                && html.contains(HEADING_RULE),
            "details / abbr / selection / caption / figure / kbd/samp / print-color-adjust / thead / heading must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "body color / 48rem / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "code chip must not regress: {html}"
        );
        assert!(
            html.contains("ul,ol{margin:0 0 1rem;padding-left:1.25rem}"),
            "ol/ul indent must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_html_tbody_row_group_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        assert_eq!(
            first_table(&doc).rows[0].cells[0].shading,
            Some(TH_BG),
            "th fill must not regress"
        );
        let html = to_html(&doc);
        assert!(
            html.contains(TBODY_RULE)
                && html.contains("tbody{display:table-row-group}")
                && html.contains(THEAD_RULE),
            "letter.md HTML must pin tbody row-group tightness: {html}"
        );
        assert!(
            html.contains("<tbody><tr><td>Input</td><td>letter.md</td><td>input SHA-256</td></tr>"),
            "letter.md hop body must sit in <tbody>: {html}"
        );
        let grouped = read(
            b"<article><table><thead><tr><th>Hop</th></tr></thead><tbody><tr><td>Input</td></tr></tbody></table></article>",
        )
        .expect("tbody html");
        let grouped_out = to_html(&grouped);
        assert!(
            grouped_out.contains(TBODY_RULE)
                && grouped_out.contains("<tbody>")
                && !grouped_out.contains("tbody{display:block")
                && !grouped_out.contains("tbody{display:none"),
            "round-trip HTML must keep letter tbody row-group, not a display:block reset: {grouped_out}"
        );
        let grouped_t = first_table(&grouped);
        assert!(grouped_t.rows[0].header, "thead row must stay a header");
        assert!(!grouped_t.rows[1].header, "tbody row must stay body");
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(
            html.contains(DETAILS_RULE) && html.contains(ABBR_RULE) && html.contains(SELECTION_RULE),
            "details / abbr / selection must not regress: {html}"
        );
        assert!(
            html.contains(CAPTION_RULE) && html.contains(FIGURE_RULE) && html.contains(KBD_SAMP_RULE),
            "caption / figure / kbd/samp must not regress: {html}"
        );
        assert!(
            html.contains(PRINT_COLOR_RULE) && html.contains(TH_RULE) && html.contains(THEAD_RULE),
            "print-color-adjust / thead fill must not regress: {html}"
        );
        assert!(
            html.contains(HEADING_RULE),
            "heading rules must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "48rem / body teal / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(
            html.contains("table{border-collapse:collapse;margin:1rem 0;width:100%}"),
            "table collapse must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
        assert!(
            !html.contains("tbody{display:block")
                && !html.contains("summary{display:list-item")
                && !html.contains("h1{font-size:2em")
                && !html.contains("color:#111827"),
            "letter.md HTML must not keep block tbody, UA disclosure summary, unruled headings, or slate #111827: {html}"
        );
    }

    #[test]
    fn article_css_td_vertical_align_top_tight() {
        let html = to_html(&Document::new());
        assert_eq!(CELL_VALIGN_RULE, "th,td{vertical-align:top}");
        assert_ne!(
            CELL_VALIGN_RULE, "th,td{vertical-align:middle}",
            "UA middle valign centers wrapped hop cells, not WeasyPrint letter top"
        );
        assert_ne!(
            CELL_VALIGN_RULE, "th,td{vertical-align:inherit}",
            "inherit valign keeps tbody middle and punches sparse row bands"
        );
        assert!(
            html.contains(CELL_VALIGN_RULE),
            "article CSS must pin th,td top so UA tbody middle does not punch the hop table: {html}"
        );
        assert!(
            html.contains("details[open]>summary{list-style:none}th,td{vertical-align:top}"),
            "cell valign must sit after details/summary in cascade order: {html}"
        );
        assert!(
            html.contains("th,td{vertical-align:top}")
                && html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem;text-align:left;font-size:.95rem}")
                && html.contains("table{border-collapse:collapse;margin:1rem 0;width:100%}")
                && html.contains(TBODY_RULE),
            "top valign must keep cell pad, collapse, and tbody row-group: {html}"
        );
        assert!(
            !html.contains("th,td{vertical-align:middle")
                && !html.contains("th,td{vertical-align:inherit")
                && !html.contains("th,td{vertical-align:baseline")
                && !html.contains("td{vertical-align:middle")
                && !html.contains("tbody{vertical-align:middle"),
            "UA/reset middle valign is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(DETAILS_RULE)
                && html.contains(TBODY_RULE)
                && html.contains(THEAD_RULE)
                && html.contains(ABBR_RULE)
                && html.contains(SELECTION_RULE)
                && html.contains(CAPTION_RULE)
                && html.contains(FIGURE_RULE)
                && html.contains(KBD_SAMP_RULE)
                && html.contains(PRINT_COLOR_RULE)
                && html.contains(TH_RULE)
                && html.contains(HEADING_RULE),
            "details / tbody / thead / abbr / selection / caption / figure / kbd/samp / print-color-adjust / heading must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "body color / 48rem / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "code chip must not regress: {html}"
        );
        assert!(
            html.contains("ul,ol{margin:0 0 1rem;padding-left:1.25rem}"),
            "ol/ul indent must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_html_td_vertical_align_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        assert_eq!(
            first_table(&doc).rows[0].cells[0].shading,
            Some(TH_BG),
            "th fill must not regress"
        );
        let html = to_html(&doc);
        assert!(
            html.contains(CELL_VALIGN_RULE)
                && html.contains("th,td{vertical-align:top}")
                && html.contains(TBODY_RULE)
                && html.contains(THEAD_RULE),
            "letter.md HTML must pin th,td top valign tightness: {html}"
        );
        assert!(
            html.contains("<tbody><tr><td>Input</td><td>letter.md</td><td>input SHA-256</td></tr>"),
            "letter.md hop body must sit in <tbody>: {html}"
        );
        let grouped = read(
            b"<article><table><thead><tr><th>Hop</th></tr></thead><tbody><tr><td>Input</td></tr></tbody></table></article>",
        )
        .expect("tbody html");
        let grouped_out = to_html(&grouped);
        assert!(
            grouped_out.contains(CELL_VALIGN_RULE)
                && grouped_out.contains("th,td{vertical-align:top}")
                && grouped_out.contains(TBODY_RULE)
                && !grouped_out.contains("th,td{vertical-align:middle")
                && !grouped_out.contains("tbody{vertical-align:middle"),
            "round-trip HTML must keep letter top valign, not UA tbody middle: {grouped_out}"
        );
        let grouped_t = first_table(&grouped);
        assert!(grouped_t.rows[0].header, "thead row must stay a header");
        assert!(!grouped_t.rows[1].header, "tbody row must stay body");
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(
            html.contains(DETAILS_RULE) && html.contains(ABBR_RULE) && html.contains(SELECTION_RULE),
            "details / abbr / selection must not regress: {html}"
        );
        assert!(
            html.contains(CAPTION_RULE) && html.contains(FIGURE_RULE) && html.contains(KBD_SAMP_RULE),
            "caption / figure / kbd/samp must not regress: {html}"
        );
        assert!(
            html.contains(PRINT_COLOR_RULE) && html.contains(TH_RULE) && html.contains(THEAD_RULE),
            "print-color-adjust / thead fill must not regress: {html}"
        );
        assert!(
            html.contains(HEADING_RULE),
            "heading rules must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "48rem / body teal / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(
            html.contains("table{border-collapse:collapse;margin:1rem 0;width:100%}"),
            "table collapse must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
        assert!(
            !html.contains("th,td{vertical-align:middle")
                && !html.contains("tbody{display:block")
                && !html.contains("summary{display:list-item")
                && !html.contains("h1{font-size:2em")
                && !html.contains("color:#111827"),
            "letter.md HTML must not keep UA middle valign, block tbody, UA disclosure summary, unruled headings, or slate #111827: {html}"
        );
    }

    #[test]
    fn article_css_table_collapse_tight() {
        let html = to_html(&Document::new());
        assert_eq!(TABLE_RULE, "table{border-collapse:collapse;margin:1rem 0;width:100%}");
        assert_ne!(
            TABLE_RULE, "table{border-collapse:separate;border-spacing:2px}",
            "WeasyPrint UA separate/2px spacing punches sparse hop-cell gaps"
        );
        assert_ne!(
            TABLE_RULE, "table{border-collapse:separate}",
            "separate collapse is UA sparse grid, not letter.html hop table"
        );
        assert!(
            TABLE_RULE.contains("border-collapse:collapse")
                && TABLE_RULE.contains("margin:1rem 0")
                && TABLE_RULE.contains("width:100%"),
            "table must stay collapsed full-width with 1rem band, not UA 2px spacing"
        );
        assert!(
            html.contains(TABLE_RULE),
            "article CSS must pin table collapse so UA border-spacing:2px does not punch the hop grid: {html}"
        );
        assert!(
            html.contains("table{border-collapse:collapse;margin:1rem 0;width:100%}thead{display:table-header-group}tbody{display:table-row-group}"),
            "table collapse must sit before thead/tbody in cascade order: {html}"
        );
        assert!(
            html.contains("table{border-collapse:collapse;margin:1rem 0;width:100%}")
                && html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem;text-align:left;font-size:.95rem}")
                && html.contains(CELL_VALIGN_RULE)
                && html.contains(TBODY_RULE)
                && html.contains(THEAD_RULE),
            "collapse must keep cell pad, top valign, and thead/tbody row-group: {html}"
        );
        assert!(
            !html.contains("table{border-collapse:separate")
                && !html.contains("table{border-spacing:2px")
                && !html.contains("border-collapse:separate;border-spacing:2px"),
            "UA/reset separate 2px spacing is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(CELL_VALIGN_RULE)
                && html.contains(DETAILS_RULE)
                && html.contains(TBODY_RULE)
                && html.contains(THEAD_RULE)
                && html.contains(ABBR_RULE)
                && html.contains(SELECTION_RULE)
                && html.contains(CAPTION_RULE)
                && html.contains(FIGURE_RULE)
                && html.contains(KBD_SAMP_RULE)
                && html.contains(PRINT_COLOR_RULE)
                && html.contains(TH_RULE)
                && html.contains(HEADING_RULE),
            "valign / details / tbody / thead / abbr / selection / caption / figure / kbd/samp / print-color-adjust / heading must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "body color / 48rem / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "code chip must not regress: {html}"
        );
        assert!(
            html.contains("ul,ol{margin:0 0 1rem;padding-left:1.25rem}"),
            "ol/ul indent must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
        assert!(
            html.contains("padding:2.5rem 2.75rem"),
            "article padding 2.5rem 2.75rem must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_html_table_collapse_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        assert_eq!(
            first_table(&doc).rows[0].cells[0].shading,
            Some(TH_BG),
            "th fill must not regress"
        );
        let html = to_html(&doc);
        assert!(
            html.contains(TABLE_RULE)
                && html.contains("table{border-collapse:collapse;margin:1rem 0;width:100%}")
                && html.contains(THEAD_RULE)
                && html.contains(TBODY_RULE)
                && html.contains(CELL_VALIGN_RULE),
            "letter.md HTML must pin table collapse tightness: {html}"
        );
        assert!(
            html.contains("<tbody><tr><td>Input</td><td>letter.md</td><td>input SHA-256</td></tr>"),
            "letter.md hop body must sit in <tbody>: {html}"
        );
        let grouped = read(
            b"<article><table><thead><tr><th>Hop</th></tr></thead><tbody><tr><td>Input</td></tr></tbody></table></article>",
        )
        .expect("table html");
        let grouped_out = to_html(&grouped);
        assert!(
            grouped_out.contains(TABLE_RULE)
                && grouped_out.contains("table{border-collapse:collapse")
                && grouped_out.contains(TBODY_RULE)
                && !grouped_out.contains("table{border-collapse:separate")
                && !grouped_out.contains("table{border-spacing:2px"),
            "round-trip HTML must keep letter table collapse, not UA separate 2px: {grouped_out}"
        );
        let grouped_t = first_table(&grouped);
        assert!(grouped_t.rows[0].header, "thead row must stay a header");
        assert!(!grouped_t.rows[1].header, "tbody row must stay body");
        assert_eq!(
            grouped_t.cell_spacing, 0,
            "collapsed hop table must keep cell_spacing 0, not UA 2px"
        );
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(
            html.contains(DETAILS_RULE) && html.contains(ABBR_RULE) && html.contains(SELECTION_RULE),
            "details / abbr / selection must not regress: {html}"
        );
        assert!(
            html.contains(CAPTION_RULE) && html.contains(FIGURE_RULE) && html.contains(KBD_SAMP_RULE),
            "caption / figure / kbd/samp must not regress: {html}"
        );
        assert!(
            html.contains(PRINT_COLOR_RULE) && html.contains(TH_RULE) && html.contains(THEAD_RULE),
            "print-color-adjust / thead fill must not regress: {html}"
        );
        assert!(
            html.contains(HEADING_RULE),
            "heading rules must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "48rem / body teal / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
        assert!(
            !html.contains("table{border-collapse:separate")
                && !html.contains("th,td{vertical-align:middle")
                && !html.contains("tbody{display:block")
                && !html.contains("summary{display:list-item")
                && !html.contains("h1{font-size:2em")
                && !html.contains("color:#111827"),
            "letter.md HTML must not keep UA separate table, middle valign, block tbody, UA disclosure summary, unruled headings, or slate #111827: {html}"
        );
    }

    #[test]
    fn article_css_list_indent_tight() {
        let html = to_html(&Document::new());
        assert_eq!(LIST_RULE, "ul,ol{margin:0 0 1rem;padding-left:1.25rem}");
        assert_ne!(
            LIST_RULE, "ul,ol{padding-left:40px}",
            "WeasyPrint UA 40px list gutter punches the 48rem column"
        );
        assert_ne!(
            LIST_RULE, "ul,ol{margin:1em 0;padding-left:40px}",
            "UA 1em/40px list box is sparse, not letter.html 1.25rem indent"
        );
        assert!(
            LIST_RULE.contains("padding-left:1.25rem")
                && LIST_RULE.contains("margin:0 0 1rem")
                && LIST_RULE.contains("ul,ol{"),
            "list indent must stay letter 1.25rem, not UA 40px"
        );
        assert_eq!(LIST_INDENT_HU, 1500, "1.25rem at 16px rem / 96dpi is 1500 HU");
        assert!(
            html.contains(LIST_RULE),
            "article CSS must pin ul,ol 1.25rem so UA padding-left:40px does not punch the 48rem column: {html}"
        );
        assert!(
            html.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}ul,ol{margin:0 0 1rem;padding-left:1.25rem}ul ul,ol ul,ul ol,ol ol{margin:.25rem 0}"),
            "list indent must sit after hr and before nested ul/ol in cascade order: {html}"
        );
        assert!(
            html.contains(LIST_RULE)
                && html.contains(NESTED_LIST_RULE)
                && html.contains("ul ul,ol ul,ul ol,ol ol{margin:.25rem 0}")
                && html.contains("li{margin:0 0 .35rem}")
                && html.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "1.25rem ul,ol must keep nested 0.25rem, li 0.35rem, and task indent: {html}"
        );
        assert!(
            !html.contains("ul,ol{padding-left:40px")
                && !html.contains("ul,ol{padding-left:2.5rem")
                && !html.contains("ul,ol{margin:1em 0;padding-left:40px")
                && !html.contains("padding-left:40px;margin:0 0 1rem"),
            "UA/reset 40px list gutter is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(TABLE_RULE)
                && html.contains(CELL_VALIGN_RULE)
                && html.contains(DETAILS_RULE)
                && html.contains(TBODY_RULE)
                && html.contains(THEAD_RULE)
                && html.contains(ABBR_RULE)
                && html.contains(SELECTION_RULE)
                && html.contains(CAPTION_RULE)
                && html.contains(FIGURE_RULE)
                && html.contains(KBD_SAMP_RULE)
                && html.contains(PRINT_COLOR_RULE)
                && html.contains(TH_RULE)
                && html.contains(HEADING_RULE),
            "table collapse / valign / details / tbody / thead / abbr / selection / caption / figure / kbd/samp / print-color-adjust / heading must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "body color / 48rem / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "code chip must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
        assert!(
            html.contains("padding:2.5rem 2.75rem"),
            "article padding 2.5rem 2.75rem must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_html_list_indent_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        assert_eq!(
            first_table(&doc).rows[0].cells[0].shading,
            Some(TH_BG),
            "th fill must not regress"
        );
        let indents: Vec<_> = doc.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) if p.numbering.as_ref().is_some_and(|n| n.level == 0) => {
                    Some(p.indent_left)
                }
                _ => None,
            })
            .collect();
        assert!(
            !indents.is_empty() && indents.iter().all(|i| *i == LIST_INDENT_HU),
            "letter.md top-level lists must keep 1.25rem indent, not UA 40px: {indents:?}"
        );
        let html = to_html(&doc);
        assert!(
            html.contains(LIST_RULE)
                && html.contains("ul,ol{margin:0 0 1rem;padding-left:1.25rem}")
                && html.contains(NESTED_LIST_RULE)
                && html.contains("ul.tasks{list-style:none;padding-left:1.25rem}")
                && html.contains("<ol>")
                && html.contains(r#"<ul class="tasks">"#),
            "letter.md HTML must pin ul,ol 1.25rem tightness: {html}"
        );
        assert!(
            html.contains("<ol><li>Hash the input</li>"),
            "letter.md agent-replay ol must stay in-flow: {html}"
        );
        let listed = read(
            b"<article><ol><li>Hash the input</li><li>Run Convert<ul><li>PDF/A</li></ul></li></ol></article>",
        )
        .expect("list html");
        let listed_out = to_html(&listed);
        assert!(
            listed_out.contains(LIST_RULE)
                && listed_out.contains("ul,ol{margin:0 0 1rem;padding-left:1.25rem")
                && !listed_out.contains("ul,ol{padding-left:40px")
                && !listed_out.contains("ul,ol{padding-left:2.5rem"),
            "round-trip HTML must keep letter 1.25rem list indent, not UA 40px: {listed_out}"
        );
        let listed_indents: Vec<_> = listed.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) if p.numbering.is_some() => Some(p.indent_left),
                _ => None,
            })
            .collect();
        assert_eq!(
            listed_indents,
            [LIST_INDENT_HU, LIST_INDENT_HU, LIST_INDENT_HU * 2],
            "round-trip lists must keep 1.25rem / nested 2.5rem, not UA 40px"
        );
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(
            html.contains(TABLE_RULE) && html.contains(CELL_VALIGN_RULE) && html.contains(TBODY_RULE),
            "table collapse / valign / tbody must not regress: {html}"
        );
        assert!(
            html.contains(DETAILS_RULE) && html.contains(ABBR_RULE) && html.contains(SELECTION_RULE),
            "details / abbr / selection must not regress: {html}"
        );
        assert!(
            html.contains(CAPTION_RULE) && html.contains(FIGURE_RULE) && html.contains(KBD_SAMP_RULE),
            "caption / figure / kbd/samp must not regress: {html}"
        );
        assert!(
            html.contains(PRINT_COLOR_RULE) && html.contains(TH_RULE) && html.contains(THEAD_RULE),
            "print-color-adjust / thead fill must not regress: {html}"
        );
        assert!(
            html.contains(HEADING_RULE),
            "heading rules must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "48rem / body teal / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
        assert!(
            !html.contains("ul,ol{padding-left:40px")
                && !html.contains("table{border-collapse:separate")
                && !html.contains("th,td{vertical-align:middle")
                && !html.contains("tbody{display:block")
                && !html.contains("summary{display:list-item")
                && !html.contains("h1{font-size:2em")
                && !html.contains("color:#111827"),
            "letter.md HTML must not keep UA 40px lists, separate table, middle valign, block tbody, UA disclosure summary, unruled headings, or slate #111827: {html}"
        );
    }

    #[test]
    fn article_css_nested_list_tight() {
        let html = to_html(&Document::new());
        assert_eq!(
            NESTED_LIST_RULE,
            "ul ul,ol ul,ul ol,ol ol{margin:.25rem 0}"
        );
        assert_ne!(
            NESTED_LIST_RULE, "ul ul,ol ul,ul ol,ol ol{margin:1em 0}",
            "WeasyPrint UA 1em nested list margin punches sparse bands in the 48rem column"
        );
        assert_ne!(
            NESTED_LIST_RULE, "ul ul,ol ul,ul ol,ol ol{margin:0}",
            "zero nested list margin jams Bay 4 / PDF/A children into the parent item"
        );
        assert!(
            NESTED_LIST_RULE.contains("margin:.25rem 0")
                && NESTED_LIST_RULE.contains("ul ul,ol ul,ul ol,ol ol{"),
            "nested lists must stay letter 0.25rem, not UA 1em or jammed 0"
        );
        assert!(
            html.contains(NESTED_LIST_RULE),
            "article CSS must pin nested ul/ol 0.25rem so UA 1em nested margins do not punch the 48rem column: {html}"
        );
        assert!(
            html.contains("ul,ol{margin:0 0 1rem;padding-left:1.25rem}ul ul,ol ul,ul ol,ol ol{margin:.25rem 0}li{margin:0 0 .35rem}"),
            "nested 0.25rem must sit after 1.25rem ul,ol and before li 0.35rem in cascade order: {html}"
        );
        assert!(
            html.contains(LIST_RULE)
                && html.contains(NESTED_LIST_RULE)
                && html.contains("ul ul,ol ul,ul ol,ol ol{margin:.25rem 0}")
                && html.contains("li{margin:0 0 .35rem}")
                && html.contains("ul.tasks{list-style:none;padding-left:1.25rem}"),
            "nested 0.25rem must keep 1.25rem ul,ol, li 0.35rem, and task indent: {html}"
        );
        assert!(
            !html.contains("ul ul,ol ul,ul ol,ol ol{margin:1em 0")
                && !html.contains("ul ul{margin:1em 0")
                && !html.contains("ul ul,ol ul,ul ol,ol ol{margin:0}")
                && !html.contains("ul ul{margin:0;"),
            "UA 1em / jammed 0 nested list margin is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(TABLE_RULE)
                && html.contains(CELL_VALIGN_RULE)
                && html.contains(DETAILS_RULE)
                && html.contains(TBODY_RULE)
                && html.contains(THEAD_RULE)
                && html.contains(ABBR_RULE)
                && html.contains(SELECTION_RULE)
                && html.contains(CAPTION_RULE)
                && html.contains(FIGURE_RULE)
                && html.contains(KBD_SAMP_RULE)
                && html.contains(PRINT_COLOR_RULE)
                && html.contains(TH_RULE)
                && html.contains(HEADING_RULE),
            "table collapse / valign / details / tbody / thead / abbr / selection / caption / figure / kbd/samp / print-color-adjust / heading must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "body color / 48rem / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"),
            "code chip must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
        assert!(
            html.contains("padding:2.5rem 2.75rem"),
            "article padding 2.5rem 2.75rem must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_html_nested_list_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        assert_eq!(
            first_table(&doc).rows[0].cells[0].shading,
            Some(TH_BG),
            "th fill must not regress"
        );
        let nested: Vec<_> = doc.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) if p.numbering.as_ref().is_some_and(|n| n.level >= 1) => {
                    Some(p.indent_left)
                }
                _ => None,
            })
            .collect();
        assert!(
            !nested.is_empty() && nested.iter().all(|i| *i == LIST_INDENT_HU * 2),
            "letter.md nested lists must keep 2.5rem indent under 1.25rem parents: {nested:?}"
        );
        let html = to_html(&doc);
        assert!(
            html.contains(NESTED_LIST_RULE)
                && html.contains("ul ul,ol ul,ul ol,ol ol{margin:.25rem 0}")
                && html.contains(LIST_RULE)
                && html.contains("<ol>")
                && html.contains(r#"<ul class="tasks">"#),
            "letter.md HTML must pin nested ul/ol 0.25rem tightness: {html}"
        );
        assert!(
            html.contains("<ul><li>Bay 4, H<sub>2</sub>O seals intact</li></ul>"),
            "letter.md task nest (Bay 4) must stay a nested ul: {html}"
        );
        assert!(
            html.contains("<ul><li>PDF/<sup>A</sup></li><li>HTML</li></ul>"),
            "letter.md agent-replay nest (PDF/A · HTML) must stay a nested ul: {html}"
        );
        let listed = read(
            b"<article><ol><li>Hash the input</li><li>Run Convert<ul><li>PDF/A</li><li>HTML</li></ul></li><li>Compare the capsule</li></ol></article>",
        )
        .expect("nested list html");
        let listed_out = to_html(&listed);
        assert!(
            listed_out.contains(NESTED_LIST_RULE)
                && listed_out.contains("ul ul,ol ul,ul ol,ol ol{margin:.25rem 0}")
                && listed_out.contains(LIST_RULE)
                && !listed_out.contains("ul ul,ol ul,ul ol,ol ol{margin:1em 0")
                && !listed_out.contains("ul ul,ol ul,ul ol,ol ol{margin:0}"),
            "round-trip HTML must keep letter 0.25rem nested list margin, not UA 1em: {listed_out}"
        );
        let listed_indents: Vec<_> = listed.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) if p.numbering.is_some() => Some(p.indent_left),
                _ => None,
            })
            .collect();
        assert_eq!(
            listed_indents,
            [
                LIST_INDENT_HU,
                LIST_INDENT_HU,
                LIST_INDENT_HU * 2,
                LIST_INDENT_HU * 2,
                LIST_INDENT_HU
            ],
            "round-trip nested lists must keep 1.25rem / 2.5rem, not UA 40px"
        );
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(
            html.contains(TABLE_RULE) && html.contains(CELL_VALIGN_RULE) && html.contains(TBODY_RULE),
            "table collapse / valign / tbody must not regress: {html}"
        );
        assert!(
            html.contains(DETAILS_RULE) && html.contains(ABBR_RULE) && html.contains(SELECTION_RULE),
            "details / abbr / selection must not regress: {html}"
        );
        assert!(
            html.contains(CAPTION_RULE) && html.contains(FIGURE_RULE) && html.contains(KBD_SAMP_RULE),
            "caption / figure / kbd/samp must not regress: {html}"
        );
        assert!(
            html.contains(PRINT_COLOR_RULE) && html.contains(TH_RULE) && html.contains(THEAD_RULE),
            "print-color-adjust / thead fill must not regress: {html}"
        );
        assert!(
            html.contains(HEADING_RULE),
            "heading rules must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "48rem / body teal / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
        assert!(
            !html.contains("ul ul,ol ul,ul ol,ol ol{margin:1em 0")
                && !html.contains("ul,ol{padding-left:40px")
                && !html.contains("table{border-collapse:separate")
                && !html.contains("th,td{vertical-align:middle")
                && !html.contains("tbody{display:block")
                && !html.contains("summary{display:list-item")
                && !html.contains("h1{font-size:2em")
                && !html.contains("color:#111827"),
            "letter.md HTML must not keep UA 1em nested lists, 40px lists, separate table, middle valign, block tbody, UA disclosure summary, unruled headings, or slate #111827: {html}"
        );
    }

    #[test]
    fn article_css_code_span_pad_tight() {
        let html = to_html(&Document::new());
        assert_eq!(
            CODE_RULE,
            "code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"
        );
        assert_ne!(
            CODE_RULE, "code{font-family:monospace}",
            "WeasyPrint UA code is bare monospace, not the letter.html prove chip"
        );
        assert_ne!(
            CODE_RULE, "code{padding:0}",
            "zero code-span pad jams `prove` into body type"
        );
        assert!(
            CODE_RULE.contains("padding:.1em .35em")
                && CODE_RULE.contains("background:#ccfbf1")
                && CODE_RULE.contains("font-size:.92em")
                && CODE_RULE.contains("border-radius:3px"),
            "code span must keep letter .1em .35em mint pad, not UA unpadded monospace"
        );
        assert!(
            html.contains(CODE_RULE),
            "article CSS must pin code-span .1em .35em pad so UA monospace does not punch the 48rem column: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}pre{background:#ccfbf1;padding:.75rem 1rem;margin:.6rem 0 1rem;border-radius:4px;overflow:auto;font-family:ui-monospace,Consolas,monospace}"),
            "code-span pad must sit after mark and before pre in cascade order: {html}"
        );
        assert!(
            html.contains(CODE_RULE)
                && html.contains("code{background:#ccfbf1;padding:.1em .35em")
                && html.contains("pre code{background:transparent;padding:0;color:#134e4a;font-family:ui-monospace,Consolas,monospace}")
                && html.contains(KBD_SAMP_RULE)
                && KBD_SAMP_RULE.contains("padding:.1em .35em"),
            "code-span pad must keep pre-code zero pad and kbd/samp matching .1em .35em: {html}"
        );
        assert!(
            !html.contains("code{font-family:monospace}")
                && !html.contains("code{padding:0}")
                && !html.contains("code{background:none")
                && !html.contains("code{padding:0;border-radius:0"),
            "UA unpadded / reset code is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(NESTED_LIST_RULE)
                && html.contains(LIST_RULE)
                && html.contains(TABLE_RULE)
                && html.contains(CELL_VALIGN_RULE)
                && html.contains(DETAILS_RULE)
                && html.contains(TBODY_RULE)
                && html.contains(THEAD_RULE)
                && html.contains(ABBR_RULE)
                && html.contains(SELECTION_RULE)
                && html.contains(CAPTION_RULE)
                && html.contains(FIGURE_RULE)
                && html.contains(PRINT_COLOR_RULE)
                && html.contains(TH_RULE)
                && html.contains(HEADING_RULE),
            "nested 0.25rem / 1.25rem lists / table collapse / valign / details / tbody / thead / abbr / selection / caption / figure / print-color-adjust / heading must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "body color / 48rem / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
        assert!(
            html.contains("padding:2.5rem 2.75rem"),
            "article padding 2.5rem 2.75rem must not regress: {html}"
        );
        assert!(
            html.contains("ul ul,ol ul,ul ol,ol ol{margin:.25rem 0}"),
            "nested list 0.25rem must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_html_code_span_pad_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("`prove`"),
            "letter.md must keep the inline code span WeasyPrint paints as a padded chip"
        );
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        assert_eq!(
            first_table(&doc).rows[0].cells[0].shading,
            Some(TH_BG),
            "th fill must not regress"
        );
        let html = to_html(&doc);
        assert!(
            html.contains(CODE_RULE)
                && html.contains("code{background:#ccfbf1;padding:.1em .35em")
                && html.contains("<code>prove</code>")
                && html.contains("pre code{background:transparent;padding:0"),
            "letter.md HTML must pin code-span .1em .35em pad tightness: {html}"
        );
        let coded = read(b"<article><p>Run <code>prove</code> twice.</p></article>").expect("code html");
        let coded_out = to_html(&coded);
        assert!(
            coded_out.contains(CODE_RULE)
                && coded_out.contains("code{background:#ccfbf1;padding:.1em .35em")
                && coded_out.contains("<code>prove</code>")
                && !coded_out.contains("code{font-family:monospace}")
                && !coded_out.contains("code{padding:0}"),
            "round-trip HTML must keep letter .1em .35em code-span pad, not UA monospace: {coded_out}"
        );
        let prove = coded.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) => p.runs.iter().find(|r| r.style.code),
                _ => None,
            })
            .expect("code run");
        assert!(
            prove.style.code && prove.style.font.as_deref() == Some(CODE_FONT),
            "round-trip code span must keep Consolas"
        );
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(
            html.contains(NESTED_LIST_RULE)
                && html.contains(LIST_RULE)
                && html.contains(TABLE_RULE)
                && html.contains(CELL_VALIGN_RULE)
                && html.contains(TBODY_RULE),
            "nested 0.25rem / 1.25rem lists / table collapse / valign / tbody must not regress: {html}"
        );
        assert!(
            html.contains(DETAILS_RULE) && html.contains(ABBR_RULE) && html.contains(SELECTION_RULE),
            "details / abbr / selection must not regress: {html}"
        );
        assert!(
            html.contains(CAPTION_RULE) && html.contains(FIGURE_RULE) && html.contains(KBD_SAMP_RULE),
            "caption / figure / kbd/samp must not regress: {html}"
        );
        assert!(
            html.contains(PRINT_COLOR_RULE) && html.contains(TH_RULE) && html.contains(THEAD_RULE),
            "print-color-adjust / thead fill must not regress: {html}"
        );
        assert!(
            html.contains(HEADING_RULE),
            "heading rules must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "48rem / body teal / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
        assert!(
            !html.contains("code{font-family:monospace}")
                && !html.contains("code{padding:0}")
                && !html.contains("ul ul,ol ul,ul ol,ol ol{margin:1em 0")
                && !html.contains("ul,ol{padding-left:40px")
                && !html.contains("table{border-collapse:separate")
                && !html.contains("th,td{vertical-align:middle")
                && !html.contains("tbody{display:block")
                && !html.contains("summary{display:list-item")
                && !html.contains("h1{font-size:2em")
                && !html.contains("color:#111827"),
            "letter.md HTML must not keep UA unpadded code, 1em nested lists, 40px lists, separate table, middle valign, block tbody, UA disclosure summary, unruled headings, or slate #111827: {html}"
        );
    }

    #[test]
    fn article_css_pre_pad_tight() {
        let html = to_html(&Document::new());
        assert_eq!(
            PRE_RULE,
            "pre{background:#ccfbf1;padding:.75rem 1rem;margin:.6rem 0 1rem;border-radius:4px;overflow:auto;font-family:ui-monospace,Consolas,monospace}"
        );
        assert_ne!(
            PRE_RULE, "pre{font-family:monospace}",
            "WeasyPrint UA pre is bare monospace with no mint pad, not the letter.html prove fence"
        );
        assert_ne!(
            PRE_RULE, "pre{padding:0}",
            "zero pre pad jams the prove fence into the mint box edge"
        );
        assert_ne!(
            PRE_RULE, "pre{margin:1em 0}",
            "WeasyPrint UA 1em pre margin is sparse, not letter .75rem 1rem pad"
        );
        assert!(
            PRE_RULE.contains("padding:.75rem 1rem")
                && PRE_RULE.contains("background:#ccfbf1")
                && PRE_RULE.contains("margin:.6rem 0 1rem")
                && PRE_RULE.contains("border-radius:4px")
                && PRE_RULE.contains("font-family:ui-monospace,Consolas,monospace"),
            "pre must keep letter .75rem 1rem mint pad, not UA unpadded monospace"
        );
        assert!(
            html.contains(PRE_RULE),
            "article CSS must pin pre .75rem 1rem pad so UA unpadded pre does not punch the 48rem column: {html}"
        );
        assert!(
            html.contains("code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}pre{background:#ccfbf1;padding:.75rem 1rem;margin:.6rem 0 1rem;border-radius:4px;overflow:auto;font-family:ui-monospace,Consolas,monospace}pre code{background:transparent;padding:0;color:#134e4a;font-family:ui-monospace,Consolas,monospace}"),
            "pre pad must sit after the code-span chip and before pre-code zero pad in cascade order: {html}"
        );
        assert!(
            html.contains(PRE_RULE)
                && html.contains("pre{background:#ccfbf1;padding:.75rem 1rem")
                && html.contains("pre code{background:transparent;padding:0;color:#134e4a;font-family:ui-monospace,Consolas,monospace}")
                && html.contains(CODE_RULE)
                && CODE_RULE.contains("padding:.1em .35em"),
            "pre pad must keep code-span .1em .35em chip and pre-code zero pad: {html}"
        );
        assert!(
            !html.contains("pre{padding:0}")
                && !html.contains("pre{margin:1em 0")
                && !html.contains("pre{font-family:monospace}")
                && !html.contains("pre{background:none")
                && !html.contains("pre{padding:0;border-radius:0"),
            "UA unpadded / 1em / reset pre is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(CODE_RULE)
                && html.contains(NESTED_LIST_RULE)
                && html.contains(LIST_RULE)
                && html.contains(TABLE_RULE)
                && html.contains(CELL_VALIGN_RULE)
                && html.contains(DETAILS_RULE)
                && html.contains(TBODY_RULE)
                && html.contains(THEAD_RULE)
                && html.contains(ABBR_RULE)
                && html.contains(SELECTION_RULE)
                && html.contains(CAPTION_RULE)
                && html.contains(FIGURE_RULE)
                && html.contains(PRINT_COLOR_RULE)
                && html.contains(TH_RULE)
                && html.contains(HEADING_RULE),
            "code-span pad / nested 0.25rem / 1.25rem lists / table collapse / valign / details / tbody / thead / abbr / selection / caption / figure / print-color-adjust / heading must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "body color / 48rem / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
        assert!(
            html.contains("padding:2.5rem 2.75rem"),
            "article padding 2.5rem 2.75rem must not regress: {html}"
        );
        assert!(
            html.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"),
            "hr 3px / 1.1rem must not regress: {html}"
        );
    }

    #[test]
    fn letter_md_html_pre_pad_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("```\ndocagent prove examples/letter.md\n```")
                || text.contains("docagent prove examples/letter.md"),
            "letter.md must keep the fenced prove command WeasyPrint paints in a padded pre"
        );
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        assert_eq!(
            first_table(&doc).rows[0].cells[0].shading,
            Some(TH_BG),
            "th fill must not regress"
        );
        let code = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) if p.code_block => Some(p),
                _ => None,
            })
            .expect("pre");
        assert_eq!(code.space_before, CODE_BEFORE_HU);
        assert_eq!(code.space_after, CODE_AFTER_HU);
        assert!(
            code.runs.iter().any(|r| {
                r.style.font.as_deref() == Some(CODE_FONT)
                    && r.style.latin_font.as_deref() == Some(CODE_FONT)
            }),
            "letter.md prove fence must stay Consolas under the .75rem 1rem pre pad"
        );
        let html = to_html(&doc);
        assert!(
            html.contains(PRE_RULE)
                && html.contains("pre{background:#ccfbf1;padding:.75rem 1rem")
                && html.contains("<pre><code>docagent prove examples/letter.md</code></pre>")
                && html.contains("pre code{background:transparent;padding:0"),
            "letter.md HTML must pin pre .75rem 1rem pad tightness: {html}"
        );
        let fenced = read(
            b"<article><pre><code>docagent prove examples/letter.md</code></pre></article>",
        )
        .expect("pre html");
        let fenced_out = to_html(&fenced);
        assert!(
            fenced_out.contains(PRE_RULE)
                && fenced_out.contains("pre{background:#ccfbf1;padding:.75rem 1rem")
                && fenced_out.contains("<pre><code>docagent prove examples/letter.md</code></pre>")
                && !fenced_out.contains("pre{font-family:monospace}")
                && !fenced_out.contains("pre{padding:0}")
                && !fenced_out.contains("pre{margin:1em 0"),
            "round-trip HTML must keep letter .75rem 1rem pre pad, not UA unpadded monospace: {fenced_out}"
        );
        let prove = fenced.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) if p.code_block => Some(p),
                _ => None,
            })
            .expect("pre block");
        assert!(
            prove.code_block
                && prove.space_before == CODE_BEFORE_HU
                && prove.space_after == CODE_AFTER_HU
                && prove
                    .runs
                    .iter()
                    .any(|r| r.style.font.as_deref() == Some(CODE_FONT)),
            "round-trip pre must keep Consolas and letter .6rem/1rem margins"
        );
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(
            html.contains(CODE_RULE) && html.contains(NESTED_LIST_RULE) && html.contains(LIST_RULE),
            "code-span pad / nested 0.25rem / 1.25rem lists must not regress: {html}"
        );
        assert!(
            html.contains(TABLE_RULE)
                && html.contains(CELL_VALIGN_RULE)
                && html.contains(TBODY_RULE),
            "table collapse / valign / tbody must not regress: {html}"
        );
        assert!(
            html.contains(DETAILS_RULE) && html.contains(ABBR_RULE) && html.contains(SELECTION_RULE),
            "details / abbr / selection must not regress: {html}"
        );
        assert!(
            html.contains(CAPTION_RULE) && html.contains(FIGURE_RULE) && html.contains(KBD_SAMP_RULE),
            "caption / figure / kbd/samp must not regress: {html}"
        );
        assert!(
            html.contains(PRINT_COLOR_RULE) && html.contains(TH_RULE) && html.contains(THEAD_RULE),
            "print-color-adjust / thead fill must not regress: {html}"
        );
        assert!(
            html.contains(HEADING_RULE),
            "heading rules must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "48rem / body teal / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
        assert!(
            !html.contains("pre{padding:0}")
                && !html.contains("pre{margin:1em 0")
                && !html.contains("pre{font-family:monospace}")
                && !html.contains("code{font-family:monospace}")
                && !html.contains("code{padding:0}")
                && !html.contains("ul ul,ol ul,ul ol,ol ol{margin:1em 0")
                && !html.contains("ul,ol{padding-left:40px")
                && !html.contains("table{border-collapse:separate")
                && !html.contains("th,td{vertical-align:middle")
                && !html.contains("tbody{display:block")
                && !html.contains("summary{display:list-item")
                && !html.contains("h1{font-size:2em")
                && !html.contains("color:#111827"),
            "letter.md HTML must not keep UA unpadded pre, 1em pre margin, unpadded code, 1em nested lists, 40px lists, separate table, middle valign, block tbody, UA disclosure summary, unruled headings, or slate #111827: {html}"
        );
    }

    #[test]
    fn article_css_img_24pt_floor_and_readable_measure() {
        let html = to_html(&Document::new());
        assert_eq!(
            IMG_RULE,
            "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
        );
        assert_eq!(MIN_IMAGE_EDGE_PT, 24);
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * HU_PER_POINT);
        assert!(
            IMG_RULE.contains("min-width:24pt")
                && IMG_RULE.contains("min-height:24pt")
                && IMG_RULE.contains("display:block")
                && IMG_RULE.contains("margin:.6rem 0"),
            "img must keep the PDF 24pt floor, not UA intrinsic 16px"
        );
        assert_ne!(
            IMG_RULE, "img{max-width:100%;height:auto}",
            "WeasyPrint UA img uses PNG 16px; min-width 24pt is the letter floor"
        );
        assert_ne!(
            IMG_RULE, "img{min-width:12pt;min-height:12pt;max-width:100%;height:auto}",
            "12pt is 16px at 96dpi and paints as a speck, not the PDF 24pt floor"
        );
        assert_eq!(
            ARTICLE_RULE,
            r#"article{max-width:48rem;margin:2rem auto;padding:2.5rem 2.75rem;background:#fff;border:1px solid #e2e8f0;font-family:"Segoe UI",Roboto,Helvetica,Arial,sans-serif;font-size:10pt;line-height:1.45;color:#134e4a}"#
        );
        assert!(
            ARTICLE_RULE.contains("max-width:48rem")
                && ARTICLE_RULE.contains("font-size:10pt")
                && ARTICLE_RULE.contains("line-height:1.45")
                && ARTICLE_RULE.contains("color:#134e4a")
                && ARTICLE_RULE.contains("padding:2.5rem 2.75rem"),
            "article CSS must stay a readable WeasyPrint letter column"
        );
        assert_ne!(
            ARTICLE_MAX_WIDTH_REM, 100,
            "100rem is a full-bleed column, not letter.html 48rem"
        );
        assert!(
            html.contains(IMG_RULE) && html.contains(ARTICLE_RULE),
            "article CSS must pin 24pt img floor + 48rem measure as WeasyPrint input: {html}"
        );
        assert!(
            html.contains("p{margin:0 0 .7rem}img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}blockquote{margin:.5rem 0 1rem"),
            "img 24pt floor must sit after p margin and before the quote bar: {html}"
        );
        assert!(
            !html.contains("img{max-width:100%;height:auto;margin:.6rem 0}")
                && !html.contains("img{min-width:12pt")
                && !html.contains("img{width:16px")
                && !html.contains("article{max-width:100rem")
                && !html.contains("article{max-width:none")
                && !html.contains("color:#111827"),
            "UA 16px img / full-bleed / slate article is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(PRE_RULE)
                && html.contains(CODE_RULE)
                && html.contains(BODY_RULE)
                && html.contains(PAGE_RULE)
                && html.contains(HEADING_RULE),
            "pre / code / body teal / @page / heading must not regress: {html}"
        );
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48);
    }

    #[test]
    fn letter_md_html_img_24pt_floor_and_readable_article() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("![Harbor mark](docs/assets/mark.png)"),
            "letter.md must keep the relative harbor-mark path"
        );
        let png = fixture_png();
        let (px_w, px_h) = png_ihdr_px(&png).expect("mark.png IHDR");
        assert_eq!((px_w, px_h), (16, 16), "fixture is 16×16 px");
        assert!(
            px_to_hu(16) < MIN_IMAGE_EDGE_HU,
            "16px at 96dpi must be below the 24pt floor"
        );
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert_eq!(img.bytes, png);
        assert_eq!(
            (img.width, img.height),
            (MIN_IMAGE_EDGE_HU, MIN_IMAGE_EDGE_HU),
            "letter.md IR must floor the harbor mark to 24pt"
        );
        let written = String::from_utf8(docagent_md::write(&doc).expect("md write")).unwrap();
        assert!(
            written.contains("![Harbor mark](docs/assets/mark.png)"),
            "markdown image path must round-trip: {written}"
        );
        assert!(
            !written.contains("data:image/png;base64,"),
            "letter.md path round-trip must not emit a data URI: {written}"
        );
        let html = to_html(&doc);
        assert!(
            html.contains(IMG_RULE)
                && html.contains(
                    "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
                ),
            "letter.md HTML must pin the PDF 24pt img floor: {html}"
        );
        assert!(
            html.contains(ARTICLE_RULE)
                && html.contains(r#"article{max-width:48rem"#)
                && html.contains("font-size:10pt;line-height:1.45")
                && html.contains("color:#134e4a"),
            "letter.md HTML must keep readable article CSS as WeasyPrint input: {html}"
        );
        let tag = first_img_tag(&html, "Harbor mark");
        assert!(
            tag.contains(r#"style="width:24pt;height:24pt""#),
            "WeasyPrint paints PNG intrinsic 16px unless the tag names 24pt: {tag}"
        );
        assert_eq!(
            img_tag_display_pt(&html, "Harbor mark"),
            Some((MIN_IMAGE_EDGE_PT, MIN_IMAGE_EDGE_PT)),
            "letter.md <img> display size must match the PDF 24pt floor: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(
            html.contains(PRE_RULE) && html.contains(CODE_RULE) && html.contains(PAGE_RULE),
            "pre / code / @page must not regress: {html}"
        );
        assert!(
            !html.contains("16px")
                && !html.contains("width:12pt")
                && !html.contains("article{max-width:100rem")
                && !html.contains("color:#111827"),
            "letter.md HTML must not size the mark from 16px/12pt or drop the 48rem column: {html}"
        );
        let again = docagent_md::read(written.as_bytes()).expect("md round-trip");
        assert_eq!(first_image(&again).bytes, png);
        let out = to_html(&again);
        assert_eq!(
            img_tag_display_pt(&out, "Harbor mark"),
            Some((MIN_IMAGE_EDGE_PT, MIN_IMAGE_EDGE_PT)),
            "path round-trip HTML must still paint at 24pt: {out}"
        );
    }

    #[test]
    fn article_css_marker_teal_tight() {
        let html = to_html(&Document::new());
        assert_eq!(MARKER_RULE, "::marker{color:#134e4a}");
        assert_ne!(
            MARKER_RULE,
            "::marker{font-variant-numeric:tabular-nums;white-space:pre;text-transform:none}",
            "WeasyPrint UA ::marker has no color, so decimals paint CanvasText black"
        );
        assert_ne!(
            MARKER_RULE, "::marker{color:black}",
            "black markers punch the teal letter, not body #134e4a"
        );
        assert_ne!(
            MARKER_RULE, "::marker{color:CanvasText}",
            "CanvasText markers are UA black, not letter.html teal"
        );
        assert!(
            MARKER_RULE.contains("::marker{color:#134e4a}"),
            "list markers must stay body teal, not UA uncolored CanvasText"
        );
        assert!(
            html.contains(MARKER_RULE),
            "article CSS must pin ::marker teal so UA CanvasText does not punch the 48rem column: {html}"
        );
        assert!(
            html.contains("th,td{vertical-align:top}::marker{color:#134e4a}"),
            "::marker teal must sit after cell valign in cascade order: {html}"
        );
        assert!(
            html.contains(MARKER_RULE)
                && html.contains("::marker{color:#134e4a}")
                && html.contains(CELL_VALIGN_RULE)
                && html.contains(LIST_RULE)
                && html.contains(NESTED_LIST_RULE),
            "marker teal must keep cell valign and 1.25rem / nested 0.25rem lists: {html}"
        );
        assert!(
            !html.contains("::marker{color:black")
                && !html.contains("::marker{color:#000")
                && !html.contains("::marker{color:CanvasText")
                && !html.contains("::marker{color:initial")
                && !html.contains("::marker{color:currentColor}"),
            "UA uncolored / black ::marker is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(PRE_RULE)
                && html.contains(CODE_RULE)
                && html.contains(NESTED_LIST_RULE)
                && html.contains(LIST_RULE)
                && html.contains(TABLE_RULE)
                && html.contains(CELL_VALIGN_RULE)
                && html.contains(DETAILS_RULE)
                && html.contains(TBODY_RULE)
                && html.contains(THEAD_RULE)
                && html.contains(ABBR_RULE)
                && html.contains(SELECTION_RULE)
                && html.contains(CAPTION_RULE)
                && html.contains(FIGURE_RULE)
                && html.contains(PRINT_COLOR_RULE)
                && html.contains(TH_RULE)
                && html.contains(HEADING_RULE),
            "pre / code-span pad / nested 0.25rem / 1.25rem lists / table collapse / valign / details / tbody / thead / abbr / selection / caption / figure / print-color-adjust / heading must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "body color / 48rem / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
        assert_eq!(BODY_FG.r, 19);
        assert_eq!(BODY_FG.g, 78);
        assert_eq!(BODY_FG.b, 74);
    }

    #[test]
    fn letter_md_html_marker_teal_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("1. Hash the input"),
            "letter.md must keep the agent-replay ol WeasyPrint paints with teal markers"
        );
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        assert_eq!(
            first_table(&doc).rows[0].cells[0].shading,
            Some(TH_BG),
            "th fill must not regress"
        );
        let decimals: Vec<_> = numbered_paras(&doc)
            .into_iter()
            .filter(|p| {
                p.numbering
                    .as_ref()
                    .is_some_and(|n| n.format == Some(NumberFormat::Decimal))
            })
            .collect();
        assert!(
            decimals.iter().any(|p| p.plain_text().contains("Hash the input")),
            "letter.md agent-replay must stay NumberFormat::Decimal: {:?}",
            decimals.iter().map(|p| p.plain_text()).collect::<Vec<_>>()
        );
        assert!(
            decimals.iter().all(|p| {
                p.runs
                    .iter()
                    .filter(|r| matches!(r.content, RunContent::Text(_)))
                    .all(|r| r.style.color == BODY_FG)
            }),
            "letter.md ol runs must stay body teal under ::marker #134e4a: {:?}",
            decimals
                .iter()
                .flat_map(|p| p.runs.iter().map(|r| r.style.color))
                .collect::<Vec<_>>()
        );
        let html = to_html(&doc);
        assert!(
            html.contains(MARKER_RULE)
                && html.contains("::marker{color:#134e4a}")
                && html.contains("<ol><li>Hash the input</li>"),
            "letter.md HTML must pin ::marker teal tightness: {html}"
        );
        let listed = read(
            b"<article><ol><li>Hash the input</li><li>Run Convert<ul><li>PDF/A</li></ul></li></ol></article>",
        )
        .expect("ol html");
        let listed_out = to_html(&listed);
        assert!(
            listed_out.contains(MARKER_RULE)
                && listed_out.contains("::marker{color:#134e4a}")
                && listed_out.contains("<ol><li>Hash the input</li>")
                && !listed_out.contains("::marker{color:black")
                && !listed_out.contains("::marker{color:CanvasText"),
            "round-trip HTML must keep letter ::marker teal, not UA CanvasText: {listed_out}"
        );
        let nums: Vec<_> = numbered_paras(&listed)
            .into_iter()
            .filter(|p| {
                p.numbering
                    .as_ref()
                    .is_some_and(|n| n.format == Some(NumberFormat::Decimal))
            })
            .collect();
        assert!(
            !nums.is_empty()
                && nums.iter().all(|p| {
                    p.runs
                        .iter()
                        .filter(|r| matches!(r.content, RunContent::Text(_)))
                        .all(|r| r.style.color == BODY_FG)
                }),
            "round-trip ol must keep NumberFormat::Decimal body teal"
        );
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(
            html.contains(PRE_RULE) && html.contains(CODE_RULE) && html.contains(NESTED_LIST_RULE),
            "pre / code-span pad / nested 0.25rem must not regress: {html}"
        );
        assert!(
            html.contains(LIST_RULE)
                && html.contains(TABLE_RULE)
                && html.contains(CELL_VALIGN_RULE)
                && html.contains(TBODY_RULE),
            "1.25rem lists / table collapse / valign / tbody must not regress: {html}"
        );
        assert!(
            html.contains(DETAILS_RULE) && html.contains(ABBR_RULE) && html.contains(SELECTION_RULE),
            "details / abbr / selection must not regress: {html}"
        );
        assert!(
            html.contains(CAPTION_RULE) && html.contains(FIGURE_RULE) && html.contains(KBD_SAMP_RULE),
            "caption / figure / kbd/samp must not regress: {html}"
        );
        assert!(
            html.contains(PRINT_COLOR_RULE) && html.contains(TH_RULE) && html.contains(THEAD_RULE),
            "print-color-adjust / thead fill must not regress: {html}"
        );
        assert!(
            html.contains(HEADING_RULE),
            "heading rules must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "48rem / body teal / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
        assert!(
            !html.contains("::marker{color:black")
                && !html.contains("::marker{color:CanvasText")
                && !html.contains("pre{padding:0}")
                && !html.contains("code{padding:0}")
                && !html.contains("ul ul,ol ul,ul ol,ol ol{margin:1em 0")
                && !html.contains("ul,ol{padding-left:40px")
                && !html.contains("table{border-collapse:separate")
                && !html.contains("th,td{vertical-align:middle")
                && !html.contains("tbody{display:block")
                && !html.contains("summary{display:list-item")
                && !html.contains("h1{font-size:2em")
                && !html.contains("color:#111827"),
            "letter.md HTML must not keep UA black markers, unpadded pre/code, 1em nested lists, 40px lists, separate table, middle valign, block tbody, UA disclosure summary, unruled headings, or slate #111827: {html}"
        );
    }

    #[test]
    fn article_css_pre_code_zero_pad_tight() {
        let html = to_html(&Document::new());
        assert_eq!(
            PRE_CODE_RULE,
            "pre code{background:transparent;padding:0;color:#134e4a;font-family:ui-monospace,Consolas,monospace}"
        );
        assert_ne!(
            PRE_CODE_RULE,
            "pre code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}",
            "inherited CODE_RULE chip pad insets the prove fence inside PRE_RULE's .75rem 1rem box"
        );
        assert_ne!(
            PRE_CODE_RULE, "pre code{padding:.1em .35em}",
            "code-span .1em .35em pad on pre code punches a sparse mint band in the fence"
        );
        assert_ne!(
            PRE_CODE_RULE, "pre code{background:#ccfbf1}",
            "mint fill on pre code doubles the pre box and punches the letter fence"
        );
        assert!(
            PRE_CODE_RULE.contains("background:transparent")
                && PRE_CODE_RULE.contains("padding:0")
                && PRE_CODE_RULE.contains("color:#134e4a")
                && PRE_CODE_RULE.contains("font-family:ui-monospace,Consolas,monospace"),
            "pre code must zero the inherited prove-chip pad, not keep mint inset"
        );
        assert!(
            html.contains(PRE_CODE_RULE),
            "article CSS must pin pre-code zero pad so CODE_RULE does not punch the prove fence: {html}"
        );
        assert!(
            html.contains("pre{background:#ccfbf1;padding:.75rem 1rem;margin:.6rem 0 1rem;border-radius:4px;overflow:auto;font-family:ui-monospace,Consolas,monospace}pre code{background:transparent;padding:0;color:#134e4a;font-family:ui-monospace,Consolas,monospace}hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"),
            "pre-code zero pad must sit after pre pad and before hr in cascade order: {html}"
        );
        assert!(
            html.contains(PRE_CODE_RULE)
                && html.contains("pre code{background:transparent;padding:0")
                && html.contains(PRE_RULE)
                && html.contains(CODE_RULE)
                && html.contains(MARKER_RULE),
            "pre-code zero pad must keep pre .75rem 1rem, code-span .1em .35em, and ::marker teal: {html}"
        );
        assert!(
            !html.contains("pre code{padding:.1em .35em")
                && !html.contains("pre code{background:#ccfbf1")
                && !html.contains("pre code{background:inherit")
                && !html.contains("pre code{padding:.1em"),
            "inherited code-span chip on pre code is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(PRE_RULE)
                && html.contains(CODE_RULE)
                && html.contains(MARKER_RULE)
                && html.contains(NESTED_LIST_RULE)
                && html.contains(LIST_RULE)
                && html.contains(TABLE_RULE)
                && html.contains(CELL_VALIGN_RULE)
                && html.contains(DETAILS_RULE)
                && html.contains(TBODY_RULE)
                && html.contains(THEAD_RULE)
                && html.contains(ABBR_RULE)
                && html.contains(SELECTION_RULE)
                && html.contains(CAPTION_RULE)
                && html.contains(FIGURE_RULE)
                && html.contains(PRINT_COLOR_RULE)
                && html.contains(TH_RULE)
                && html.contains(HEADING_RULE)
                && html.contains(KBD_SAMP_RULE),
            "pre / code-span pad / marker teal / nested 0.25rem / 1.25rem lists / table collapse / valign / details / tbody / thead / abbr / selection / caption / figure / print-color-adjust / heading / kbd/samp must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "body color / 48rem / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
        assert!(
            html.contains("th,td{vertical-align:top}::marker{color:#134e4a}"),
            "marker teal after valign must not regress: {html}"
        );
        assert_eq!(BODY_FG.r, 19);
        assert_eq!(BODY_FG.g, 78);
        assert_eq!(BODY_FG.b, 74);
    }

    #[test]
    fn letter_md_html_pre_code_zero_pad_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("docagent prove examples/letter.md"),
            "letter.md must keep the fenced prove command WeasyPrint paints in pre code"
        );
        assert!(
            text.contains("1. Hash the input"),
            "letter.md must keep the agent-replay ol WeasyPrint paints with teal markers"
        );
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        assert_eq!(
            first_table(&doc).rows[0].cells[0].shading,
            Some(TH_BG),
            "th fill must not regress"
        );
        let fence = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) if p.code_block => Some(p),
                _ => None,
            })
            .expect("pre");
        assert!(
            fence.code_block
                && fence.space_before == CODE_BEFORE_HU
                && fence.space_after == CODE_AFTER_HU
                && fence.runs.iter().any(|r| {
                    r.style.font.as_deref() == Some(CODE_FONT)
                        && r.style.latin_font.as_deref() == Some(CODE_FONT)
                        && matches!(&r.content, RunContent::Text(t) if t.contains("prove"))
                }),
            "letter.md prove fence must stay Consolas under pre-code zero pad"
        );
        let decimals: Vec<_> = numbered_paras(&doc)
            .into_iter()
            .filter(|p| {
                p.numbering
                    .as_ref()
                    .is_some_and(|n| n.format == Some(NumberFormat::Decimal))
            })
            .collect();
        assert!(
            decimals
                .iter()
                .any(|p| p.plain_text().contains("Hash the input")),
            "letter.md agent-replay must stay NumberFormat::Decimal: {:?}",
            decimals.iter().map(|p| p.plain_text()).collect::<Vec<_>>()
        );
        assert!(
            decimals.iter().all(|p| {
                p.runs
                    .iter()
                    .filter(|r| matches!(r.content, RunContent::Text(_)))
                    .all(|r| r.style.color == BODY_FG)
            }),
            "letter.md ol runs must stay body teal under ::marker #134e4a"
        );
        let html = to_html(&doc);
        assert!(
            html.contains(PRE_CODE_RULE)
                && html.contains("pre code{background:transparent;padding:0")
                && html.contains("<pre><code>docagent prove examples/letter.md</code></pre>")
                && html.contains(MARKER_RULE)
                && html.contains("<ol><li>Hash the input</li>"),
            "letter.md HTML must pin pre-code zero pad tightness: {html}"
        );
        let fenced = read(
            b"<article><pre><code>docagent prove examples/letter.md</code></pre></article>",
        )
        .expect("pre html");
        let fenced_out = to_html(&fenced);
        assert!(
            fenced_out.contains(PRE_CODE_RULE)
                && fenced_out.contains("pre code{background:transparent;padding:0")
                && fenced_out.contains("<pre><code>docagent prove examples/letter.md</code></pre>")
                && !fenced_out.contains("pre code{padding:.1em .35em")
                && !fenced_out.contains("pre code{background:#ccfbf1"),
            "round-trip HTML must keep letter pre-code zero pad, not inherited chip pad: {fenced_out}"
        );
        let prove = fenced.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) if p.code_block => Some(p),
                _ => None,
            })
            .expect("pre block");
        assert!(
            prove.code_block
                && prove.space_before == CODE_BEFORE_HU
                && prove.space_after == CODE_AFTER_HU
                && prove
                    .runs
                    .iter()
                    .any(|r| r.style.font.as_deref() == Some(CODE_FONT)),
            "round-trip pre must keep Consolas and letter .6rem/1rem margins"
        );
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(
            html.contains(PRE_RULE) && html.contains(CODE_RULE) && html.contains(MARKER_RULE),
            "pre pad / code-span pad / marker teal must not regress: {html}"
        );
        assert!(
            html.contains(NESTED_LIST_RULE)
                && html.contains(LIST_RULE)
                && html.contains(TABLE_RULE)
                && html.contains(CELL_VALIGN_RULE)
                && html.contains(TBODY_RULE),
            "nested 0.25rem / 1.25rem lists / table collapse / valign / tbody must not regress: {html}"
        );
        assert!(
            html.contains(DETAILS_RULE) && html.contains(ABBR_RULE) && html.contains(SELECTION_RULE),
            "details / abbr / selection must not regress: {html}"
        );
        assert!(
            html.contains(CAPTION_RULE) && html.contains(FIGURE_RULE) && html.contains(KBD_SAMP_RULE),
            "caption / figure / kbd/samp must not regress: {html}"
        );
        assert!(
            html.contains(PRINT_COLOR_RULE) && html.contains(TH_RULE) && html.contains(THEAD_RULE),
            "print-color-adjust / thead fill must not regress: {html}"
        );
        assert!(
            html.contains(HEADING_RULE),
            "heading rules must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "48rem / body teal / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
        let written = String::from_utf8(docagent_md::write(&doc).expect("md write")).unwrap();
        assert!(
            written.contains("![Harbor mark](docs/assets/mark.png)"),
            "letter.md image path must round-trip: {written}"
        );
        assert!(
            !html.contains("pre code{padding:.1em .35em")
                && !html.contains("pre code{background:#ccfbf1")
                && !html.contains("::marker{color:black")
                && !html.contains("::marker{color:CanvasText")
                && !html.contains("pre{padding:0}")
                && !html.contains("code{padding:0}")
                && !html.contains("ul ul,ol ul,ul ol,ol ol{margin:1em 0")
                && !html.contains("ul,ol{padding-left:40px")
                && !html.contains("table{border-collapse:separate")
                && !html.contains("th,td{vertical-align:middle")
                && !html.contains("tbody{display:block")
                && !html.contains("summary{display:list-item")
                && !html.contains("h1{font-size:2em")
                && !html.contains("color:#111827"),
            "letter.md HTML must not keep inherited pre-code chip pad, UA black markers, unpadded pre/code, 1em nested lists, 40px lists, separate table, middle valign, block tbody, UA disclosure summary, unruled headings, or slate #111827: {html}"
        );
    }

    #[test]
    fn article_css_hr_rule_tight() {
        let html = to_html(&Document::new());
        assert_eq!(
            HR_RULE,
            "hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"
        );
        assert_ne!(
            HR_RULE, "hr{color:gray;border-style:inset;border-width:1px;margin:0.5em auto}",
            "WeasyPrint UA 1px inset gray hr is a sparse hairline, not letter 3px teal"
        );
        assert_ne!(
            HR_RULE, "hr{border:1px inset}",
            "UA 1px inset hr punches a gray band between the hop table and confirm list"
        );
        assert_ne!(
            HR_RULE, "hr{margin:.5em auto}",
            "UA 0.5em auto hr margin is sparse, not letter 1.1rem"
        );
        assert!(
            HR_RULE.contains("border:none")
                && HR_RULE.contains("border-top:3px solid #134e4a")
                && HR_RULE.contains("margin:1.1rem 0"),
            "hr must keep letter 3px body-teal rule and 1.1rem gap, not UA inset gray"
        );
        assert!(
            html.contains(HR_RULE),
            "article CSS must pin hr 3px teal so UA 1px inset does not punch the 48rem column: {html}"
        );
        assert!(
            html.contains("pre code{background:transparent;padding:0;color:#134e4a;font-family:ui-monospace,Consolas,monospace}hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}ul,ol{margin:0 0 1rem;padding-left:1.25rem}"),
            "hr 3px teal must sit after pre-code zero pad and before 1.25rem lists in cascade order: {html}"
        );
        assert!(
            html.contains(HR_RULE)
                && html.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}")
                && html.contains(PRE_CODE_RULE)
                && html.contains(PRE_RULE)
                && html.contains(CODE_RULE)
                && html.contains(MARKER_RULE),
            "hr 3px teal must keep pre-code zero pad, pre .75rem 1rem, code-span .1em .35em, and ::marker teal: {html}"
        );
        assert!(
            !html.contains("hr{border:1px")
                && !html.contains("hr{border-style:inset")
                && !html.contains("hr{margin:.5em")
                && !html.contains("hr{margin:0.5em")
                && !html.contains("hr{color:gray"),
            "UA 1px inset / 0.5em gray hr is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(PRE_RULE)
                && html.contains(CODE_RULE)
                && html.contains(PRE_CODE_RULE)
                && html.contains(MARKER_RULE)
                && html.contains(NESTED_LIST_RULE)
                && html.contains(LIST_RULE)
                && html.contains(TABLE_RULE)
                && html.contains(CELL_VALIGN_RULE)
                && html.contains(DETAILS_RULE)
                && html.contains(TBODY_RULE)
                && html.contains(THEAD_RULE)
                && html.contains(ABBR_RULE)
                && html.contains(SELECTION_RULE)
                && html.contains(CAPTION_RULE)
                && html.contains(FIGURE_RULE)
                && html.contains(PRINT_COLOR_RULE)
                && html.contains(TH_RULE)
                && html.contains(HEADING_RULE)
                && html.contains(KBD_SAMP_RULE),
            "pre / code-span pad / pre-code zero pad / marker teal / nested 0.25rem / 1.25rem lists / table collapse / valign / details / tbody / thead / abbr / selection / caption / figure / print-color-adjust / heading / kbd/samp must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "body color / 48rem / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
        assert!(
            html.contains("th,td{vertical-align:top}::marker{color:#134e4a}"),
            "marker teal after valign must not regress: {html}"
        );
        assert_eq!(BODY_FG.r, 19);
        assert_eq!(BODY_FG.g, 78);
        assert_eq!(BODY_FG.b, 74);
    }

    #[test]
    fn letter_md_html_hr_rule_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("\n---\n"),
            "letter.md must keep the thematic break WeasyPrint paints as a 3px teal hr"
        );
        assert!(
            text.contains("docagent prove examples/letter.md"),
            "letter.md must keep the fenced prove command WeasyPrint paints in pre code"
        );
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        assert_eq!(
            first_table(&doc).rows[0].cells[0].shading,
            Some(TH_BG),
            "th fill must not regress"
        );
        assert!(
            doc.sections[0]
                .body
                .iter()
                .any(|b| matches!(b, Block::Break(BreakKind::Thematic))),
            "letter.md IR must keep BreakKind::Thematic under hr 3px teal"
        );
        let html = to_html(&doc);
        assert!(
            html.contains(HR_RULE)
                && html.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}")
                && html.contains("<hr/>")
                && html.contains(PRE_CODE_RULE)
                && html.contains(MARKER_RULE),
            "letter.md HTML must pin hr 3px teal tightness: {html}"
        );
        let broken = read(b"<article><p>before</p><hr/><p>after</p></article>").expect("hr html");
        let broken_out = to_html(&broken);
        assert!(
            broken_out.contains(HR_RULE)
                && broken_out.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}")
                && broken_out.contains("<hr/>")
                && !broken_out.contains("hr{border:1px")
                && !broken_out.contains("hr{border-style:inset")
                && !broken_out.contains("hr{margin:.5em"),
            "round-trip HTML must keep letter 3px teal hr, not UA 1px inset: {broken_out}"
        );
        assert!(
            broken.sections[0]
                .body
                .iter()
                .any(|b| matches!(b, Block::Break(BreakKind::Thematic))),
            "round-trip <hr> must stay BreakKind::Thematic"
        );
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(
            html.contains(PRE_RULE)
                && html.contains(CODE_RULE)
                && html.contains(PRE_CODE_RULE)
                && html.contains(MARKER_RULE),
            "pre pad / code-span pad / pre-code zero pad / marker teal must not regress: {html}"
        );
        assert!(
            html.contains(NESTED_LIST_RULE)
                && html.contains(LIST_RULE)
                && html.contains(TABLE_RULE)
                && html.contains(CELL_VALIGN_RULE)
                && html.contains(TBODY_RULE),
            "nested 0.25rem / 1.25rem lists / table collapse / valign / tbody must not regress: {html}"
        );
        assert!(
            html.contains(DETAILS_RULE) && html.contains(ABBR_RULE) && html.contains(SELECTION_RULE),
            "details / abbr / selection must not regress: {html}"
        );
        assert!(
            html.contains(CAPTION_RULE) && html.contains(FIGURE_RULE) && html.contains(KBD_SAMP_RULE),
            "caption / figure / kbd/samp must not regress: {html}"
        );
        assert!(
            html.contains(PRINT_COLOR_RULE) && html.contains(TH_RULE) && html.contains(THEAD_RULE),
            "print-color-adjust / thead fill must not regress: {html}"
        );
        assert!(
            html.contains(HEADING_RULE),
            "heading rules must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "48rem / body teal / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
        let written = String::from_utf8(docagent_md::write(&doc).expect("md write")).unwrap();
        assert!(
            written.contains("![Harbor mark](docs/assets/mark.png)"),
            "letter.md image path must round-trip: {written}"
        );
        assert!(
            written.contains("---"),
            "markdown write must keep the thematic break: {written}"
        );
        assert!(
            !html.contains("hr{border:1px")
                && !html.contains("hr{border-style:inset")
                && !html.contains("hr{margin:.5em")
                && !html.contains("pre code{padding:.1em .35em")
                && !html.contains("pre code{background:#ccfbf1")
                && !html.contains("::marker{color:black")
                && !html.contains("::marker{color:CanvasText")
                && !html.contains("pre{padding:0}")
                && !html.contains("code{padding:0}")
                && !html.contains("ul ul,ol ul,ul ol,ol ol{margin:1em 0")
                && !html.contains("ul,ol{padding-left:40px")
                && !html.contains("table{border-collapse:separate")
                && !html.contains("th,td{vertical-align:middle")
                && !html.contains("tbody{display:block")
                && !html.contains("summary{display:list-item")
                && !html.contains("h1{font-size:2em")
                && !html.contains("color:#111827"),
            "letter.md HTML must not keep UA 1px inset hr, inherited pre-code chip pad, UA black markers, unpadded pre/code, 1em nested lists, 40px lists, separate table, middle valign, block tbody, UA disclosure summary, unruled headings, or slate #111827: {html}"
        );
    }

    #[test]
    fn article_css_li_margin_tight() {
        let html = to_html(&Document::new());
        assert_eq!(LI_RULE, "li{margin:0 0 .35rem}");
        assert_ne!(
            LI_RULE, "li{display:list-item}",
            "WeasyPrint UA li has no after-margin, so agent-replay / task items jam"
        );
        assert_ne!(
            LI_RULE, "li{margin:0}",
            "zero li margin jams Hash the input / Bay 4 into the next item"
        );
        assert_ne!(
            LI_RULE, "li{margin:1em 0}",
            "UA 1em li margin is sparse, not letter 0.35rem"
        );
        assert!(
            LI_RULE.contains("margin:0 0 .35rem") && LI_RULE.contains("li{"),
            "li must keep letter 0.35rem after-gap, not UA jammed list-item"
        );
        assert_eq!(
            LIST_ITEM_AFTER_HU, 420,
            "0.35rem at 16px rem / 96dpi is 420 HU"
        );
        assert!(
            html.contains(LI_RULE),
            "article CSS must pin li 0.35rem so UA jammed list-item does not punch the 48rem column: {html}"
        );
        assert!(
            html.contains("ul ul,ol ul,ul ol,ol ol{margin:.25rem 0}li{margin:0 0 .35rem}ul.tasks{list-style:none;padding-left:1.25rem}"),
            "li 0.35rem must sit after nested 0.25rem and before task ul in cascade order: {html}"
        );
        assert!(
            html.contains(LI_RULE)
                && html.contains("li{margin:0 0 .35rem}")
                && html.contains(HR_RULE)
                && html.contains(PRE_CODE_RULE)
                && html.contains(PRE_RULE)
                && html.contains(CODE_RULE)
                && html.contains(MARKER_RULE),
            "li 0.35rem must keep hr 3px teal, pre-code zero pad, pre .75rem 1rem, code-span .1em .35em, and ::marker teal: {html}"
        );
        assert!(
            !html.contains("li{margin:0;}")
                && !html.contains("li{margin:1em")
                && !html.contains("li{display:list-item}")
                && !html.contains("li{margin:0 0 0}"),
            "UA jammed / 1em li margin is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(PRE_RULE)
                && html.contains(CODE_RULE)
                && html.contains(PRE_CODE_RULE)
                && html.contains(HR_RULE)
                && html.contains(MARKER_RULE)
                && html.contains(NESTED_LIST_RULE)
                && html.contains(LIST_RULE)
                && html.contains(TABLE_RULE)
                && html.contains(CELL_VALIGN_RULE)
                && html.contains(DETAILS_RULE)
                && html.contains(TBODY_RULE)
                && html.contains(THEAD_RULE)
                && html.contains(ABBR_RULE)
                && html.contains(SELECTION_RULE)
                && html.contains(CAPTION_RULE)
                && html.contains(FIGURE_RULE)
                && html.contains(PRINT_COLOR_RULE)
                && html.contains(TH_RULE)
                && html.contains(HEADING_RULE)
                && html.contains(KBD_SAMP_RULE),
            "pre / code-span pad / pre-code zero pad / hr 3px / marker teal / nested 0.25rem / 1.25rem lists / table collapse / valign / details / tbody / thead / abbr / selection / caption / figure / print-color-adjust / heading / kbd/samp must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "body color / 48rem / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
        assert!(
            html.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"),
            "hr 3px teal must not regress: {html}"
        );
        assert_eq!(BODY_FG.r, 19);
        assert_eq!(BODY_FG.g, 78);
        assert_eq!(BODY_FG.b, 74);
    }

    #[test]
    fn letter_md_html_li_margin_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("- [x] Receiving dock is clear"),
            "letter.md must keep the task list WeasyPrint paints at li 0.35rem"
        );
        assert!(
            text.contains("1. Hash the input"),
            "letter.md must keep the agent-replay ol WeasyPrint paints at li 0.35rem"
        );
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        assert_eq!(
            first_table(&doc).rows[0].cells[0].shading,
            Some(TH_BG),
            "th fill must not regress"
        );
        let items = numbered_paras(&doc);
        assert!(
            items.iter().any(|p| {
                p.plain_text().contains("Hash the input") && p.space_after == LIST_ITEM_AFTER_HU
            }),
            "letter.md Hash the input must keep li 0.35rem (420 HU): {:?}",
            items.iter().map(|p| (p.plain_text(), p.space_after)).collect::<Vec<_>>()
        );
        assert!(
            items.iter().any(|p| {
                p.plain_text().contains("Bay 4") && p.space_after == LIST_ITEM_AFTER_HU
            }),
            "letter.md Bay 4 nest must keep li 0.35rem (420 HU)"
        );
        let html = to_html(&doc);
        assert!(
            html.contains(LI_RULE)
                && html.contains("li{margin:0 0 .35rem}")
                && html.contains("<ol><li>Hash the input</li>")
                && html.contains(r#"<ul class="tasks">"#)
                && html.contains(HR_RULE)
                && html.contains(MARKER_RULE),
            "letter.md HTML must pin li 0.35rem tightness: {html}"
        );
        let listed = read(
            b"<article><ol><li>Hash the input</li><li>Run Convert<ul><li>PDF/A</li><li>HTML</li></ul></li><li>Compare the capsule</li></ol></article>",
        )
        .expect("li html");
        let listed_out = to_html(&listed);
        assert!(
            listed_out.contains(LI_RULE)
                && listed_out.contains("li{margin:0 0 .35rem}")
                && listed_out.contains("<ol><li>Hash the input</li>")
                && !listed_out.contains("li{margin:0;}")
                && !listed_out.contains("li{margin:1em")
                && !listed_out.contains("li{display:list-item}"),
            "round-trip HTML must keep letter 0.35rem li gap, not UA jammed list-item: {listed_out}"
        );
        let listed_after: Vec<_> = numbered_paras(&listed)
            .into_iter()
            .map(|p| p.space_after)
            .collect();
        assert!(
            listed_after.iter().any(|a| *a == LIST_ITEM_AFTER_HU)
                && listed_after.last() == Some(&LIST_AFTER_HU),
            "round-trip ol must keep 0.35rem item gap and 1rem after the last item: {listed_after:?}"
        );
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(
            html.contains(PRE_RULE)
                && html.contains(CODE_RULE)
                && html.contains(PRE_CODE_RULE)
                && html.contains(HR_RULE)
                && html.contains(MARKER_RULE),
            "pre pad / code-span pad / pre-code zero pad / hr 3px / marker teal must not regress: {html}"
        );
        assert!(
            html.contains(NESTED_LIST_RULE)
                && html.contains(LIST_RULE)
                && html.contains(TABLE_RULE)
                && html.contains(CELL_VALIGN_RULE)
                && html.contains(TBODY_RULE),
            "nested 0.25rem / 1.25rem lists / table collapse / valign / tbody must not regress: {html}"
        );
        assert!(
            html.contains(DETAILS_RULE) && html.contains(ABBR_RULE) && html.contains(SELECTION_RULE),
            "details / abbr / selection must not regress: {html}"
        );
        assert!(
            html.contains(CAPTION_RULE) && html.contains(FIGURE_RULE) && html.contains(KBD_SAMP_RULE),
            "caption / figure / kbd/samp must not regress: {html}"
        );
        assert!(
            html.contains(PRINT_COLOR_RULE) && html.contains(TH_RULE) && html.contains(THEAD_RULE),
            "print-color-adjust / thead fill must not regress: {html}"
        );
        assert!(
            html.contains(HEADING_RULE),
            "heading rules must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "48rem / body teal / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
        let written = String::from_utf8(docagent_md::write(&doc).expect("md write")).unwrap();
        assert!(
            written.contains("![Harbor mark](docs/assets/mark.png)"),
            "letter.md image path must round-trip: {written}"
        );
        assert!(
            written.contains("1. Hash the input") && written.contains("- [x] Receiving dock is clear"),
            "markdown write must keep the agent-replay ol and task list: {written}"
        );
        assert!(
            !html.contains("li{margin:0;}")
                && !html.contains("li{margin:1em")
                && !html.contains("li{display:list-item}")
                && !html.contains("hr{border:1px")
                && !html.contains("hr{border-style:inset")
                && !html.contains("pre code{padding:.1em .35em")
                && !html.contains("pre code{background:#ccfbf1")
                && !html.contains("::marker{color:black")
                && !html.contains("::marker{color:CanvasText")
                && !html.contains("pre{padding:0}")
                && !html.contains("code{padding:0}")
                && !html.contains("ul ul,ol ul,ul ol,ol ol{margin:1em 0")
                && !html.contains("ul,ol{padding-left:40px")
                && !html.contains("table{border-collapse:separate")
                && !html.contains("th,td{vertical-align:middle")
                && !html.contains("tbody{display:block")
                && !html.contains("summary{display:list-item")
                && !html.contains("h1{font-size:2em")
                && !html.contains("color:#111827"),
            "letter.md HTML must not keep UA jammed li, 1px inset hr, inherited pre-code chip pad, UA black markers, unpadded pre/code, 1em nested lists, 40px lists, separate table, middle valign, block tbody, UA disclosure summary, unruled headings, or slate #111827: {html}"
        );
    }

    #[test]
    fn article_css_tasks_tight() {
        let html = to_html(&Document::new());
        assert_eq!(
            TASKS_RULE,
            "ul.tasks{list-style:none;padding-left:1.25rem}ul.tasks>li{list-style:none}ul.tasks ul{list-style:disc}ul.tasks input[type=checkbox]{appearance:none;-webkit-appearance:none;display:inline-block;width:.9em;height:.9em;margin-right:.4rem;vertical-align:middle;border:1.5pt solid #134e4a;border-radius:2px;background:#fff;accent-color:#134e4a}ul.tasks input[type=checkbox]:checked{background:#134e4a}"
        );
        assert_ne!(
            TASKS_RULE, "ul.tasks{list-style:disc;padding-left:40px}",
            "WeasyPrint UA disc + 40px gutter paints beside GFM checkboxes and punches the 48rem column"
        );
        assert_ne!(
            TASKS_RULE, "ul{list-style:disc;padding-left:40px}",
            "UA ul disc/40px is not letter.html ul.tasks 1.25rem no-disc"
        );
        assert!(
            TASKS_RULE.contains("ul.tasks{list-style:none;padding-left:1.25rem}")
                && TASKS_RULE.contains("ul.tasks>li{list-style:none}")
                && TASKS_RULE.contains("ul.tasks ul{list-style:disc}")
                && TASKS_RULE.contains("border:1.5pt solid #134e4a")
                && TASKS_RULE.contains("ul.tasks input[type=checkbox]:checked{background:#134e4a}"),
            "task ul must keep no-disc 1.25rem, nested Bay 4 disc, and 1.5pt teal checkbox chip"
        );
        assert_eq!(
            LIST_INDENT_HU, 1500,
            "1.25rem at 16px rem / 96dpi is 1500 HU"
        );
        assert!(
            html.contains(TASKS_RULE),
            "article CSS must pin ul.tasks so UA discs / 40px gutters do not punch the 48rem column: {html}"
        );
        assert!(
            html.contains("li{margin:0 0 .35rem}ul.tasks{list-style:none;padding-left:1.25rem}"),
            "ul.tasks must sit after li 0.35rem in cascade order: {html}"
        );
        assert!(
            html.contains("ul ul,ol ul,ul ol,ol ol{margin:.25rem 0}li{margin:0 0 .35rem}ul.tasks{list-style:none;padding-left:1.25rem}"),
            "ul.tasks must sit after nested 0.25rem / li 0.35rem in cascade order: {html}"
        );
        assert!(
            html.contains("ul.tasks input[type=checkbox]:checked{background:#134e4a}table{border-collapse:collapse;margin:1rem 0;width:100%}"),
            "ul.tasks must sit before table collapse in cascade order: {html}"
        );
        assert!(
            html.contains(TASKS_RULE)
                && html.contains("ul.tasks{list-style:none;padding-left:1.25rem}")
                && html.contains(LI_RULE)
                && html.contains(HR_RULE)
                && html.contains(PRE_CODE_RULE)
                && html.contains(PRE_RULE)
                && html.contains(CODE_RULE)
                && html.contains(MARKER_RULE),
            "ul.tasks must keep li 0.35rem, hr 3px teal, pre-code zero pad, pre .75rem 1rem, code-span .1em .35em, and ::marker teal: {html}"
        );
        assert!(
            !html.contains("ul.tasks{list-style:disc")
                && !html.contains("ul.tasks{padding-left:40px")
                && !html.contains("ul.tasks input{display:none")
                && !html.contains("input[type=checkbox]{display:none")
                && !html.contains("input[type=checkbox]{visibility:hidden")
                && !html.contains("input[type=checkbox]{opacity:0"),
            "UA disc / 40px / hidden checkbox is not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(PRE_RULE)
                && html.contains(CODE_RULE)
                && html.contains(PRE_CODE_RULE)
                && html.contains(HR_RULE)
                && html.contains(MARKER_RULE)
                && html.contains(LI_RULE)
                && html.contains(NESTED_LIST_RULE)
                && html.contains(LIST_RULE)
                && html.contains(TABLE_RULE)
                && html.contains(CELL_VALIGN_RULE)
                && html.contains(DETAILS_RULE)
                && html.contains(TBODY_RULE)
                && html.contains(THEAD_RULE)
                && html.contains(ABBR_RULE)
                && html.contains(SELECTION_RULE)
                && html.contains(CAPTION_RULE)
                && html.contains(FIGURE_RULE)
                && html.contains(PRINT_COLOR_RULE)
                && html.contains(TH_RULE)
                && html.contains(HEADING_RULE)
                && html.contains(KBD_SAMP_RULE),
            "pre / code-span pad / pre-code zero pad / hr 3px / marker teal / li 0.35rem / nested 0.25rem / 1.25rem lists / table collapse / valign / details / tbody / thead / abbr / selection / caption / figure / print-color-adjust / heading / kbd/samp must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "body color / 48rem / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
        assert!(
            html.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"),
            "hr 3px teal must not regress: {html}"
        );
        assert_eq!(BODY_FG.r, 19);
        assert_eq!(BODY_FG.g, 78);
        assert_eq!(BODY_FG.b, 74);
        assert_eq!(LI_RULE, "li{margin:0 0 .35rem}");
        assert_eq!(LIST_ITEM_AFTER_HU, 420);
    }

    #[test]
    fn letter_md_html_tasks_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("- [x] Receiving dock is clear"),
            "letter.md must keep the task list WeasyPrint paints as ul.tasks"
        );
        assert!(
            text.contains("- [ ] Replay the convert instead of hoping"),
            "letter.md must keep the unchecked GFM task WeasyPrint paints as ul.tasks"
        );
        assert!(
            text.contains("Bay 4"),
            "letter.md must keep nested Bay 4 WeasyPrint paints as ul.tasks ul disc"
        );
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        assert_eq!(
            first_table(&doc).rows[0].cells[0].shading,
            Some(TH_BG),
            "th fill must not regress"
        );
        let tasks: Vec<_> = numbered_paras(&doc)
            .into_iter()
            .filter(|p| {
                p.numbering
                    .as_ref()
                    .is_some_and(|n| n.format == Some(NumberFormat::Task))
            })
            .collect();
        assert!(
            tasks.iter().any(|p| {
                p.plain_text().contains("Receiving dock")
                    && p.indent_left == LIST_INDENT_HU
                    && p.space_after == LIST_ITEM_AFTER_HU
                    && p.numbering.as_ref().is_some_and(|n| n.start == Some(1))
            }),
            "letter.md Receiving dock must stay NumberFormat::Task at 1.25rem / 0.35rem: {:?}",
            tasks
                .iter()
                .map(|p| (p.plain_text(), p.indent_left, p.space_after))
                .collect::<Vec<_>>()
        );
        assert!(
            tasks.iter().any(|p| {
                p.plain_text().contains("Replay the convert")
                    && p.indent_left == LIST_INDENT_HU
                    && p.numbering.as_ref().is_some_and(|n| n.start.is_none())
            }),
            "letter.md Replay must stay unchecked NumberFormat::Task at 1.25rem"
        );
        let html = to_html(&doc);
        assert!(
            html.contains(TASKS_RULE)
                && html.contains("ul.tasks{list-style:none;padding-left:1.25rem}")
                && html.contains(r#"<ul class="tasks">"#)
                && html.contains(
                    r#"<input type="checkbox" disabled="" checked=""/>Receiving dock is clear"#
                )
                && html.contains(LI_RULE)
                && html.contains(HR_RULE)
                && html.contains(MARKER_RULE),
            "letter.md HTML must pin ul.tasks tightness: {html}"
        );
        let tasked = read(
            b"<article><ul class=\"tasks\"><li><input type=\"checkbox\" disabled=\"\" checked=\"\"/>Receiving dock is clear<ul><li>Bay 4</li></ul></li><li><input type=\"checkbox\" disabled=\"\"/>Replay the convert instead of hoping</li></ul></article>",
        )
        .expect("tasks html");
        let tasked_out = to_html(&tasked);
        assert!(
            tasked_out.contains(TASKS_RULE)
                && tasked_out.contains("ul.tasks{list-style:none;padding-left:1.25rem}")
                && tasked_out.contains(r#"<ul class="tasks">"#)
                && tasked_out.contains(
                    r#"<input type="checkbox" disabled="" checked=""/>Receiving dock is clear"#
                )
                && !tasked_out.contains("ul.tasks{list-style:disc")
                && !tasked_out.contains("ul.tasks{padding-left:40px")
                && !tasked_out.contains("ul.tasks input{display:none"),
            "round-trip HTML must keep letter ul.tasks, not UA disc/40px: {tasked_out}"
        );
        let tasked_items: Vec<_> = numbered_paras(&tasked)
            .into_iter()
            .filter(|p| {
                p.numbering
                    .as_ref()
                    .is_some_and(|n| n.format == Some(NumberFormat::Task))
            })
            .collect();
        assert!(
            tasked_items.iter().any(|p| {
                p.plain_text().contains("Receiving dock")
                    && p.indent_left == LIST_INDENT_HU
                    && p.numbering.as_ref().is_some_and(|n| n.start == Some(1))
            }) && tasked_items.iter().any(|p| {
                p.plain_text().contains("Replay the convert")
                    && p.numbering.as_ref().is_some_and(|n| n.start.is_none())
            }),
            "round-trip ul.tasks must keep NumberFormat::Task at 1.25rem: {:?}",
            tasked_items
                .iter()
                .map(|p| (p.plain_text(), p.indent_left, p.numbering.as_ref().map(|n| n.start)))
                .collect::<Vec<_>>()
        );
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(
            html.contains(PRE_RULE)
                && html.contains(CODE_RULE)
                && html.contains(PRE_CODE_RULE)
                && html.contains(HR_RULE)
                && html.contains(MARKER_RULE)
                && html.contains(LI_RULE),
            "pre pad / code-span pad / pre-code zero pad / hr 3px / marker teal / li 0.35rem must not regress: {html}"
        );
        assert!(
            html.contains(NESTED_LIST_RULE)
                && html.contains(LIST_RULE)
                && html.contains(TABLE_RULE)
                && html.contains(CELL_VALIGN_RULE)
                && html.contains(TBODY_RULE),
            "nested 0.25rem / 1.25rem lists / table collapse / valign / tbody must not regress: {html}"
        );
        assert!(
            html.contains(DETAILS_RULE) && html.contains(ABBR_RULE) && html.contains(SELECTION_RULE),
            "details / abbr / selection must not regress: {html}"
        );
        assert!(
            html.contains(CAPTION_RULE) && html.contains(FIGURE_RULE) && html.contains(KBD_SAMP_RULE),
            "caption / figure / kbd/samp must not regress: {html}"
        );
        assert!(
            html.contains(PRINT_COLOR_RULE) && html.contains(TH_RULE) && html.contains(THEAD_RULE),
            "print-color-adjust / thead fill must not regress: {html}"
        );
        assert!(
            html.contains(HEADING_RULE),
            "heading rules must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "48rem / body teal / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
        let written = String::from_utf8(docagent_md::write(&doc).expect("md write")).unwrap();
        assert!(
            written.contains("![Harbor mark](docs/assets/mark.png)"),
            "letter.md image path must round-trip: {written}"
        );
        assert!(
            written.contains("- [x] Receiving dock is clear")
                && written.contains("- [ ] Replay the convert instead of hoping"),
            "markdown write must keep the GFM task list: {written}"
        );
        assert!(
            !html.contains("ul.tasks{list-style:disc")
                && !html.contains("ul.tasks{padding-left:40px")
                && !html.contains("ul.tasks input{display:none")
                && !html.contains("li{margin:0;}")
                && !html.contains("li{margin:1em")
                && !html.contains("li{display:list-item}")
                && !html.contains("hr{border:1px")
                && !html.contains("hr{border-style:inset")
                && !html.contains("pre code{padding:.1em .35em")
                && !html.contains("pre code{background:#ccfbf1")
                && !html.contains("::marker{color:black")
                && !html.contains("::marker{color:CanvasText")
                && !html.contains("pre{padding:0}")
                && !html.contains("code{padding:0}")
                && !html.contains("ul ul,ol ul,ul ol,ol ol{margin:1em 0")
                && !html.contains("ul,ol{padding-left:40px")
                && !html.contains("table{border-collapse:separate")
                && !html.contains("th,td{vertical-align:middle")
                && !html.contains("tbody{display:block")
                && !html.contains("summary{display:list-item")
                && !html.contains("h1{font-size:2em")
                && !html.contains("color:#111827"),
            "letter.md HTML must not keep UA disc tasks, 40px task gutter, hidden checkboxes, jammed li, 1px inset hr, inherited pre-code chip pad, UA black markers, unpadded pre/code, 1em nested lists, 40px lists, separate table, middle valign, block tbody, UA disclosure summary, unruled headings, or slate #111827: {html}"
        );
    }

    #[test]
    fn article_css_cell_pad_tight() {
        let html = to_html(&Document::new());
        assert_eq!(
            CELL_RULE,
            "th,td{border:1px solid #cbd5e1;padding:.4rem .65rem;text-align:left;font-size:.95rem}"
        );
        assert_ne!(
            CELL_RULE, "td,th{padding:1px}",
            "WeasyPrint UA td,th 1px padding jams hop cells with no slate grid"
        );
        assert_ne!(
            CELL_RULE, "th,td{padding:0}",
            "zero cell padding is a jammed hop grid, not letter .4rem .65rem"
        );
        assert_ne!(
            CELL_RULE, "th,td{border:none;padding:1px}",
            "UA 1px / no-border is not letter 1px #cbd5e1 + .4rem .65rem"
        );
        assert!(
            CELL_RULE.contains("padding:.4rem .65rem")
                && CELL_RULE.contains("border:1px solid #cbd5e1")
                && CELL_RULE.contains("text-align:left")
                && CELL_RULE.contains("font-size:.95rem"),
            "hop cells must keep letter 0.4rem 0.65rem pad, 1px slate border, left align, .95rem type"
        );
        assert_eq!(
            CELL_PAD_X_HU, 780,
            "0.65rem at 16px rem / 96dpi is 780 HU"
        );
        assert_eq!(
            CELL_PAD_Y_HU, 480,
            "0.4rem at 16px rem / 96dpi is 480 HU"
        );
        assert!(
            html.contains(CELL_RULE),
            "article CSS must pin th,td pad so UA 1px does not jam the hop grid: {html}"
        );
        assert!(
            html.contains("tbody{display:table-row-group}th,td{border:1px solid #cbd5e1;padding:.4rem .65rem;text-align:left;font-size:.95rem}thead th,tr:first-child th{background:#ccfbf1;color:#134e4a;font-weight:700;border-bottom:2px solid #0d9488}"),
            "cell pad must sit after tbody row-group and before thead th fill in cascade order: {html}"
        );
        assert!(
            html.contains("ul.tasks input[type=checkbox]:checked{background:#134e4a}table{border-collapse:collapse;margin:1rem 0;width:100%}thead{display:table-header-group}tbody{display:table-row-group}th,td{border:1px solid #cbd5e1;padding:.4rem .65rem;text-align:left;font-size:.95rem}"),
            "cell pad must sit after ul.tasks / table collapse / thead / tbody in cascade order: {html}"
        );
        assert!(
            html.contains(CELL_RULE)
                && html.contains("th,td{border:1px solid #cbd5e1;padding:.4rem .65rem;text-align:left;font-size:.95rem}")
                && html.contains(TASKS_RULE)
                && html.contains(LI_RULE)
                && html.contains(HR_RULE)
                && html.contains(PRE_CODE_RULE)
                && html.contains(PRE_RULE)
                && html.contains(CODE_RULE)
                && html.contains(MARKER_RULE),
            "cell pad must keep ul.tasks, li 0.35rem, hr 3px teal, pre-code zero pad, pre .75rem 1rem, code-span .1em .35em, and ::marker teal: {html}"
        );
        assert!(
            !html.contains("th,td{padding:1px")
                && !html.contains("td,th{padding:1px")
                && !html.contains("th,td{padding:0}")
                && !html.contains("th,td{border:none")
                && !html.contains("th,td{border:0")
                && !html.contains("th,td{padding:.4rem .65rem;border:none"),
            "UA 1px / zero pad / no-border hop cells are not WeasyPrint letter input: {html}"
        );
        assert!(
            html.contains(PRE_RULE)
                && html.contains(CODE_RULE)
                && html.contains(PRE_CODE_RULE)
                && html.contains(HR_RULE)
                && html.contains(MARKER_RULE)
                && html.contains(LI_RULE)
                && html.contains(TASKS_RULE)
                && html.contains(NESTED_LIST_RULE)
                && html.contains(LIST_RULE)
                && html.contains(TABLE_RULE)
                && html.contains(CELL_VALIGN_RULE)
                && html.contains(DETAILS_RULE)
                && html.contains(TBODY_RULE)
                && html.contains(THEAD_RULE)
                && html.contains(ABBR_RULE)
                && html.contains(SELECTION_RULE)
                && html.contains(CAPTION_RULE)
                && html.contains(FIGURE_RULE)
                && html.contains(PRINT_COLOR_RULE)
                && html.contains(TH_RULE)
                && html.contains(HEADING_RULE)
                && html.contains(KBD_SAMP_RULE),
            "pre / code-span pad / pre-code zero pad / hr 3px / marker teal / li 0.35rem / ul.tasks / nested 0.25rem / 1.25rem lists / table collapse / valign / details / tbody / thead / abbr / selection / caption / figure / print-color-adjust / heading / kbd/samp must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "body color / 48rem / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
        assert!(
            html.contains("hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"),
            "hr 3px teal must not regress: {html}"
        );
        assert_eq!(BODY_FG.r, 19);
        assert_eq!(BODY_FG.g, 78);
        assert_eq!(BODY_FG.b, 74);
        assert_eq!(
            TASKS_RULE,
            "ul.tasks{list-style:none;padding-left:1.25rem}ul.tasks>li{list-style:none}ul.tasks ul{list-style:disc}ul.tasks input[type=checkbox]{appearance:none;-webkit-appearance:none;display:inline-block;width:.9em;height:.9em;margin-right:.4rem;vertical-align:middle;border:1.5pt solid #134e4a;border-radius:2px;background:#fff;accent-color:#134e4a}ul.tasks input[type=checkbox]:checked{background:#134e4a}"
        );
        assert_eq!(LI_RULE, "li{margin:0 0 .35rem}");
        assert_eq!(LIST_ITEM_AFTER_HU, 420);
    }

    #[test]
    fn letter_md_html_cell_pad_keeps_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("| Hop | File | Proof |"),
            "letter.md must keep the hop table WeasyPrint paints at th,td .4rem .65rem"
        );
        assert!(
            text.contains("| Input | letter.md | input SHA-256 |"),
            "letter.md must keep the Input hop row WeasyPrint paints at td .4rem .65rem"
        );
        let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let t = first_table(&doc);
        assert_eq!(
            t.rows[0].cells[0].shading,
            Some(TH_BG),
            "th fill must not regress"
        );
        for row in &t.rows {
            for cell in &row.cells {
                assert_eq!(
                    cell.padding.left, CELL_PAD_X_HU,
                    "letter.md hop cell pad x must stay 0.65rem (780 HU), not UA 1px: {}",
                    cell.padding.left
                );
                assert_eq!(cell.padding.right, CELL_PAD_X_HU);
                assert_eq!(
                    cell.padding.top, CELL_PAD_Y_HU,
                    "letter.md hop cell pad y must stay 0.4rem (480 HU), not UA 1px: {}",
                    cell.padding.top
                );
                assert_eq!(cell.padding.bottom, CELL_PAD_Y_HU);
            }
        }
        let html = to_html(&doc);
        assert!(
            html.contains(CELL_RULE)
                && html.contains(
                    "th,td{border:1px solid #cbd5e1;padding:.4rem .65rem;text-align:left;font-size:.95rem}"
                )
                && html.contains("<th><strong>Hop</strong></th>")
                && html.contains("<td>letter.md</td>")
                && html.contains(TASKS_RULE)
                && html.contains(LI_RULE)
                && html.contains(HR_RULE)
                && html.contains(MARKER_RULE),
            "letter.md HTML must pin th,td .4rem .65rem tightness: {html}"
        );
        let hopped = read(
            b"<article><table><thead><tr><th>Hop</th><th>File</th></tr></thead><tbody><tr><td>Input</td><td>letter.md</td></tr></tbody></table></article>",
        )
        .expect("cell html");
        let hopped_out = to_html(&hopped);
        assert!(
            hopped_out.contains(CELL_RULE)
                && hopped_out.contains(
                    "th,td{border:1px solid #cbd5e1;padding:.4rem .65rem;text-align:left;font-size:.95rem}"
                )
                && hopped_out.contains("<th><strong>Hop</strong></th>")
                && hopped_out.contains("<td>letter.md</td>")
                && !hopped_out.contains("th,td{padding:1px")
                && !hopped_out.contains("td,th{padding:1px")
                && !hopped_out.contains("th,td{padding:0}")
                && !hopped_out.contains("th,td{border:none"),
            "round-trip HTML must keep letter .4rem .65rem cell pad, not UA 1px: {hopped_out}"
        );
        let hopped_t = first_table(&hopped);
        for row in &hopped_t.rows {
            for cell in &row.cells {
                assert_eq!(cell.padding.left, CELL_PAD_X_HU);
                assert_eq!(cell.padding.right, CELL_PAD_X_HU);
                assert_eq!(cell.padding.top, CELL_PAD_Y_HU);
                assert_eq!(cell.padding.bottom, CELL_PAD_Y_HU);
            }
        }
        assert!(html.contains("<code>prove</code>"), "{html}");
        assert!(
            html.contains(PRE_RULE)
                && html.contains(CODE_RULE)
                && html.contains(PRE_CODE_RULE)
                && html.contains(HR_RULE)
                && html.contains(MARKER_RULE)
                && html.contains(LI_RULE)
                && html.contains(TASKS_RULE),
            "pre pad / code-span pad / pre-code zero pad / hr 3px / marker teal / li 0.35rem / ul.tasks must not regress: {html}"
        );
        assert!(
            html.contains(NESTED_LIST_RULE)
                && html.contains(LIST_RULE)
                && html.contains(TABLE_RULE)
                && html.contains(CELL_VALIGN_RULE)
                && html.contains(TBODY_RULE),
            "nested 0.25rem / 1.25rem lists / table collapse / valign / tbody must not regress: {html}"
        );
        assert!(
            html.contains(DETAILS_RULE) && html.contains(ABBR_RULE) && html.contains(SELECTION_RULE),
            "details / abbr / selection must not regress: {html}"
        );
        assert!(
            html.contains(CAPTION_RULE) && html.contains(FIGURE_RULE) && html.contains(KBD_SAMP_RULE),
            "caption / figure / kbd/samp must not regress: {html}"
        );
        assert!(
            html.contains(PRINT_COLOR_RULE) && html.contains(TH_RULE) && html.contains(THEAD_RULE),
            "print-color-adjust / thead fill must not regress: {html}"
        );
        assert!(
            html.contains(HEADING_RULE),
            "heading rules must not regress: {html}"
        );
        assert!(
            html.contains(LINK_RULE)
                && html.contains(LINK_VISITED_RULE)
                && html.contains(LINK_HOVER_RULE)
                && html.contains(LINK_FOCUS_RULE),
            "LVHF must not regress: {html}"
        );
        assert!(
            html.contains(BODY_RULE) && html.contains(ARTICLE_RULE) && html.contains(PAGE_RULE),
            "48rem / body teal / @page must not regress: {html}"
        );
        assert!(
            html.contains(
                "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
            ),
            "24pt img floor must not regress: {html}"
        );
        assert!(html.contains(r#"<img alt="Harbor mark""#), "{html}");
        assert!(
            html.contains("mark{background:#fde68a;padding:.05em .2em;color:#134e4a}"),
            "mark teal must not regress: {html}"
        );
        let written = String::from_utf8(docagent_md::write(&doc).expect("md write")).unwrap();
        assert!(
            written.contains("![Harbor mark](docs/assets/mark.png)"),
            "letter.md image path must round-trip: {written}"
        );
        assert!(
            written.contains("| **Hop** | **File** | **Proof** |")
                && written.contains("| Input | letter.md | input SHA-256 |"),
            "markdown write must keep the hop table: {written}"
        );
        assert!(
            !html.contains("th,td{padding:1px")
                && !html.contains("td,th{padding:1px")
                && !html.contains("th,td{padding:0}")
                && !html.contains("th,td{border:none")
                && !html.contains("ul.tasks{list-style:disc")
                && !html.contains("ul.tasks{padding-left:40px")
                && !html.contains("ul.tasks input{display:none")
                && !html.contains("li{margin:0;}")
                && !html.contains("li{margin:1em")
                && !html.contains("li{display:list-item}")
                && !html.contains("hr{border:1px")
                && !html.contains("hr{border-style:inset")
                && !html.contains("pre code{padding:.1em .35em")
                && !html.contains("pre code{background:#ccfbf1")
                && !html.contains("::marker{color:black")
                && !html.contains("::marker{color:CanvasText")
                && !html.contains("pre{padding:0}")
                && !html.contains("code{padding:0}")
                && !html.contains("ul ul,ol ul,ul ol,ol ol{margin:1em 0")
                && !html.contains("ul,ol{padding-left:40px")
                && !html.contains("table{border-collapse:separate")
                && !html.contains("th,td{vertical-align:middle")
                && !html.contains("tbody{display:block")
                && !html.contains("summary{display:list-item")
                && !html.contains("h1{font-size:2em")
                && !html.contains("color:#111827"),
            "letter.md HTML must not keep UA 1px cell pad, no-border hop grid, disc tasks, 40px task gutter, hidden checkboxes, jammed li, 1px inset hr, inherited pre-code chip pad, UA black markers, unpadded pre/code, 1em nested lists, 40px lists, separate table, middle valign, block tbody, UA disclosure summary, unruled headings, or slate #111827: {html}"
        );
    }

    #[test]
    fn read_rejects_non_utf8() {
        assert!(matches!(read(&[0xFF, 0xFE, 0x00]), Err(Error::NotHtml)));
    }
}
