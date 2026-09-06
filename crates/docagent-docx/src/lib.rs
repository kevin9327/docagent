//! DOCX WordprocessingML codec (ECMA-376).
//!
//! Paragraph/run/table map onto `w:p` / `w:r` / `w:t` / `w:tbl`. Twips in the
//! package are converted with `twips_to_hu` / `hu_to_twips` (factor 5).

#![forbid(unsafe_code)]

use std::collections::{BTreeSet, HashMap};
use std::io::{Cursor, Read, Write};

use docagent_model::{
    Alignment, Block, Border, BorderStyle, BreakKind, CharStyle, Color, Diagnostic, DiagnosticCode,
    Document, Float, Hu, ImageData, InlineObject, NumberFormat, NumberingRef, PageSetup, Paragraph,
    Run, RunContent, Section, Severity, Table, TableBorders, TableCell, TableRow, Underline,
    WrapMode, DEFAULT_FONT_SIZE_HU, DEFAULT_MARGIN_HU, HU_PER_INCH, HU_PER_POINT, hu_to_twips,
    twips_to_hu,
};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use thiserror::Error;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

#[derive(Debug, Error)]
pub enum Error {
    #[error("not a DOCX package")]
    NotDocx,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("xml: {0}")]
    Xml(String),
}

pub fn sniff(bytes: &[u8]) -> bool {
    if bytes.len() < 4 || bytes[0..2] != [0x50, 0x4B] {
        return false;
    }
    ZipArchive::new(Cursor::new(bytes))
        .ok()
        .map(|mut z| z.by_name("word/document.xml").is_ok())
        .unwrap_or(false)
}

struct Rel {
    target: String,
    ty: String,
}

struct MediaPart {
    bytes: Vec<u8>,
    mime: String,
}

/// 1 HWPUNIT = 1/7200 in; 1 inch = 914400 EMU; 914400/7200 = 127.
const EMU_PER_HU: i64 = 127;
/// Smallest on-page image edge (24pt). 16px at 96dpi is 12pt — too small in Word.
const MIN_IMAGE_EDGE_HU: Hu = 24 * HU_PER_POINT;
const _: () = assert!(MIN_IMAGE_EDGE_HU >= HU_PER_INCH / 4);
/// Default cell inset X when IR padding is 0. Letter CSS
/// `th,td{padding:.4rem .65rem}` at 16px rem / 96dpi: 0.65rem = 780 HU.
const CELL_PAD_X_HU: Hu = 780;
/// Default cell inset Y when IR padding is 0. 0.4rem = 480 HU.
const CELL_PAD_Y_HU: Hu = 480;
/// Gap after an image-only block. Letter CSS `p{margin:0 0 .7rem}` collapsed
/// with `img{margin:.6rem 0}` is 0.7rem = 840 HU = 168 twips. Word Normal 8pt
/// (160) jams the Harbor mark against the next body vs WeasyPrint.
const IMAGE_AFTER_HU: Hu = 840;
/// Gap before an image-only block. Letter CSS `img{margin:.6rem 0}` top is
/// 0.6rem = 720 HU = 144 twips. Word 0 / IR 80 / H2 4pt jams the Harbor mark
/// under the H2 rule vs WeasyPrint (python-docx / Pandoc omit before).
const IMAGE_BEFORE_HU: Hu = 720;
/// Letter CSS `p{margin:0 0 .7rem}` at 16px rem / 96dpi: 0.7rem = 840 HU =
/// 168 twips. Word 2013 Normal `w:spacing w:after="160"` (8pt) jams consecutive
/// body vs WeasyPrint (python-docx / Pandoc inherit Normal). Same strut as
/// [`IMAGE_AFTER_HU`] (collapsed p+img).
const BODY_AFTER_HU: Hu = 840;
const _: () = assert!(IMAGE_AFTER_HU == 840);
const _: () = assert!(IMAGE_BEFORE_HU == 720);
const _: () = assert!(IMAGE_BEFORE_HU < IMAGE_AFTER_HU);
const _: () = assert!(BODY_AFTER_HU == 840);
const _: () = assert!(BODY_AFTER_HU == IMAGE_AFTER_HU);
/// Word 2013 Normal line 1.15 (`w:line="276"` twentieths of a point).
const BODY_LINE: i32 = 276;
const _: () = assert!(BODY_LINE == 276);
/// Letter CSS `h1,h2{line-height:1.2}`. OOXML auto line is 240ths (288).
/// Inherited Normal 276 (1.15) jams heading leading vs WeasyPrint.
const HEADING_LINE: i32 = 288;
const _: () = assert!(HEADING_LINE == 288);
const _: () = assert!(HEADING_LINE > BODY_LINE);
/// Word 2013 Heading 1 before 12pt (240 twips).
const HEADING1_BEFORE_HU: Hu = 1200;
/// 8pt under H1 so Heading1+Heading2 is type, not a stacked outline.
const HEADING1_AFTER_HU: Hu = 800;
/// Word 2007 Heading 2 before 10pt (200 twips). Word 2013's 40 twips is sparse.
const HEADING2_BEFORE_HU: Hu = 1000;
const HEADING2_AFTER_HU: Hu = 400;
/// Word Heading 3 keeps a modest 4pt strut.
const HEADING3_BEFORE_HU: Hu = 400;
const HEADING3_AFTER_HU: Hu = 400;
/// Word Heading 1 16pt (`w:sz` 32). Markdown CSS 1.75rem must not override this.
const HEADING1_SIZE_HU: Hu = 16 * HU_PER_POINT;
/// Word Heading 2 13pt (`w:sz` 26).
const HEADING2_SIZE_HU: Hu = 13 * HU_PER_POINT;
/// Word Heading 3 12pt (`w:sz` 24).
const HEADING3_SIZE_HU: Hu = 12 * HU_PER_POINT;
const _: () = assert!(HEADING1_SIZE_HU == 1600);
const _: () = assert!(HEADING2_SIZE_HU == 1300);
const _: () = assert!(HEADING3_SIZE_HU == 1200);
/// Letter CSS `h1{letter-spacing:-.02em}` at Word Heading 1 16pt = -0.32pt.
/// OOXML rPr `w:spacing` is twentieths of a point (−6). 0 / missing is
/// WeasyPrint tracking dropped (python-docx / Pandoc). H2 stays 0.
const HEADING1_SPACING: i32 = -6;
const _: () = assert!(HEADING1_SPACING == -6);
/// Letter CSS `h1{border-bottom:3px solid #134e4a}` at 96dpi = 225 HU. OOXML sz 18.
const HEADING1_RULE_HU: Hu = 225;
/// Letter CSS `h1{padding-bottom:.4rem}` ≈ 5pt between type and the rule.
const HEADING1_BDR_SPACE_PT: i32 = 5;
/// Letter CSS `h2{border-bottom:2px solid #0d9488}` at 96dpi = 150 HU. OOXML sz 12.
const HEADING2_RULE_HU: Hu = 150;
/// Letter CSS `h2{padding-bottom:.2rem}` ≈ 2pt between type and the rule.
const HEADING2_BDR_SPACE_PT: i32 = 2;
/// Letter CSS `table{margin:1rem 0}` = 1200 HU = 240 twips. Word 8pt after
/// the code_block jams the hop grid against the table.
const TABLE_MARGIN_HU: Hu = 1200;
const _: () = assert!(HEADING1_RULE_HU == 225);
const _: () = assert!(HEADING2_RULE_HU == 150);
const _: () = assert!(HEADING1_BDR_SPACE_PT == 5);
const _: () = assert!(HEADING2_BDR_SPACE_PT == 2);
const _: () = assert!(TABLE_MARGIN_HU == 1200);
/// Letter CSS `ul,ol{padding-left:1.25rem}` at 16px rem / 96dpi: 1.25rem = 1500 HU
/// = 300 twips. Word 2013 / python-docx 0.5in (720) is a sparse gutter vs WeasyPrint.
const LIST_LEFT_TWIPS: i32 = 300;
/// Hanging label inside the 1.25rem gutter. Word 0.25in (360) with 0.5in left
/// puts text at 0.25in — past letter 1.25rem (0.208in). 240 twips (1rem) keeps
/// the disc / "1." in the gutter and tabs to the 1.25rem text start.
const LIST_HANGING_TWIPS: i32 = 240;
const _: () = assert!(LIST_LEFT_TWIPS == 300);
const _: () = assert!(LIST_HANGING_TWIPS == 240);
const _: () = assert!(LIST_HANGING_TWIPS < LIST_LEFT_TWIPS);
/// Letter CSS `li{margin:0 0 .35rem}` = 420 HU = 84 twips. Word
/// `w:contextualSpacing` (no extra after) jams consecutive items vs WeasyPrint.
const LIST_ITEM_AFTER_HU: Hu = 420;
/// Letter CSS `ul ul,ol ul,ul ol,ol ol{margin:.25rem 0}` at 16px rem / 96dpi:
/// 0.25rem = 300 HU = 60 twips. Sibling [`LIST_ITEM_AFTER_HU`] 0.35rem is the
/// wrong nested strut vs WeasyPrint (python-docx / Pandoc inherit 0.35rem).
const LIST_NESTED_GAP_HU: Hu = 300;
/// Letter CSS `ul,ol{margin:0 0 1rem}` after the last item = 1200 HU = 240 twips.
/// Word Normal 8pt (160) jams the list against the next body vs WeasyPrint
/// (python-docx / Pandoc inherit Normal). Consecutive items stay 0.35rem.
const LIST_AFTER_HU: Hu = 1200;
const _: () = assert!(LIST_ITEM_AFTER_HU == 420);
const _: () = assert!(LIST_NESTED_GAP_HU == 300);
const _: () = assert!(LIST_AFTER_HU == 1200);
const _: () = assert!(LIST_NESTED_GAP_HU < LIST_ITEM_AFTER_HU);
const _: () = assert!(LIST_ITEM_AFTER_HU < LIST_AFTER_HU);
const _: () = assert!(LIST_ITEM_AFTER_HU < BODY_AFTER_HU);
const _: () = assert!(LIST_AFTER_HU > BODY_AFTER_HU);
const _: () = assert!(HEADING1_AFTER_HU < BODY_AFTER_HU);
const _: () = assert!(LIST_ITEM_AFTER_HU < IMAGE_AFTER_HU);
const _: () = assert!(IMAGE_BEFORE_HU < IMAGE_AFTER_HU);
const _: () = assert!(IMAGE_AFTER_HU < LIST_AFTER_HU);
/// Letter CSS `blockquote{margin:.5rem 0 1rem}` top is 0.5rem = 120 twips.
const QUOTE_BEFORE_HU: Hu = 600;
/// Letter CSS `blockquote{margin:.5rem 0 1rem}` bottom is 1rem = 1200 HU =
/// 240 twips. Word Normal 8pt (160) jams the quote against the prove fence
/// vs WeasyPrint (python-docx / Pandoc inherit Normal). Same strut as
/// [`LIST_AFTER_HU`] / [`TABLE_MARGIN_HU`].
const QUOTE_AFTER_HU: Hu = 1200;
/// Word Quote left indent 0.5in (720 twips), not the 144-twip stub.
const QUOTE_LEFT_TWIPS: i32 = 720;
/// Letter CSS `blockquote{border-left:4px solid #134e4a}` at 96dpi = 300 HU.
/// OOXML `w:sz` is eighths of a point (24), not Word Quote auto/gray hairline 6.
const QUOTE_BAR_HU: Hu = 4 * (HU_PER_INCH / 96);
/// Letter CSS `blockquote{color:#134e4a}` (same teal as the 4px bar).
const QUOTE_COLOR: &str = "134E4A";
/// Letter body type `#134e4a` (quote/th/s). Word UA/theme is black.
const BODY_COLOR: &str = "134E4A";
/// Letter CSS `article{font-family:"Segoe UI",Roboto,Helvetica,Arial,sans-serif}`.
/// Word theme Calibri is not WeasyPrint letter input (python-docx / Pandoc).
const BODY_FONT: &str = "Segoe UI";
/// Letter CSS `h1` inherits `#134e4a`; `h2{color:#134e4a}`. Word Heading `2F5496` is Calibri blue.
const HEADING_COLOR: &str = "134E4A";
/// Word Quote `w:pBdr` left `w:space` (points). Direct so style-only Quote is not a missing bar.
const QUOTE_BDR_SPACE_PT: i32 = 4;
const _: () = assert!(QUOTE_BEFORE_HU == 600);
const _: () = assert!(QUOTE_AFTER_HU == 1200);
const _: () = assert!(QUOTE_AFTER_HU == LIST_AFTER_HU);
const _: () = assert!(QUOTE_AFTER_HU == TABLE_MARGIN_HU);
const _: () = assert!(QUOTE_AFTER_HU > QUOTE_BEFORE_HU);
const _: () = assert!(QUOTE_AFTER_HU > BODY_AFTER_HU);
const _: () = assert!(QUOTE_BAR_HU == 300);
const _: () = assert!(QUOTE_BDR_SPACE_PT == 4);
const _: () = assert!(BODY_COLOR.as_bytes()[0] == b'1' && BODY_COLOR.as_bytes()[5] == b'A');
const _: () = assert!(BODY_FONT.as_bytes()[0] == b'S' && BODY_FONT.as_bytes()[6] == b'U');
const _: () = assert!(HEADING_COLOR.as_bytes()[0] == b'1' && HEADING_COLOR.as_bytes()[5] == b'A');
/// Letter CSS `pre{margin:.6rem 0 1rem}` top is 0.6rem = 720 HU = 144 twips.
/// Word 6pt (120) / IR 80 jams the prove fence under the quote vs WeasyPrint
/// (python-docx / Pandoc inherit Normal). Same strut as [`IMAGE_BEFORE_HU`].
const CODE_BEFORE_HU: Hu = 720;
/// Letter CSS `pre{margin:.6rem 0 1rem}` bottom is 1rem = 1200 HU = 240 twips.
/// Word Normal 8pt (160) jams the prove fence against the hop table / next
/// body vs WeasyPrint (python-docx / Pandoc inherit Normal). Same strut as
/// [`QUOTE_AFTER_HU`] / [`LIST_AFTER_HU`] / [`TABLE_MARGIN_HU`].
const CODE_AFTER_HU: Hu = 1200;
/// Letter CSS `pre{padding:.75rem 1rem}` left/right = 1rem = 1200 HU = 240 twips.
/// Direct `w:ind` so Word pads the Consolas wash even if CodeBlock is ignored.
const CODE_PAD_X_HU: Hu = 1200;
/// Letter CSS `pre{padding:.75rem 1rem}` top/bottom = 0.75rem = 900 HU.
/// OOXML `w:sz` is eighths of a point (72 = 9pt). Same-fill thick pBdr is the
/// Word pad — paragraph `w:shd` does not include spacing before/after.
const CODE_PAD_Y_HU: Hu = 900;
/// OOXML eighths of a point for [`CODE_PAD_Y_HU`] (9pt).
const CODE_PAD_Y_SZ: i32 = 72;
const _: () = assert!(CODE_BEFORE_HU == 720);
const _: () = assert!(CODE_BEFORE_HU == IMAGE_BEFORE_HU);
const _: () = assert!(CODE_BEFORE_HU > QUOTE_BEFORE_HU);
const _: () = assert!(CODE_AFTER_HU == 1200);
const _: () = assert!(CODE_AFTER_HU == QUOTE_AFTER_HU);
const _: () = assert!(CODE_AFTER_HU == LIST_AFTER_HU);
const _: () = assert!(CODE_AFTER_HU == TABLE_MARGIN_HU);
const _: () = assert!(CODE_AFTER_HU > CODE_BEFORE_HU);
const _: () = assert!(CODE_AFTER_HU > BODY_AFTER_HU);
const _: () = assert!(CODE_AFTER_HU > IMAGE_AFTER_HU);
const _: () = assert!(CODE_PAD_X_HU == 1200);
const _: () = assert!(CODE_PAD_Y_HU == 900);
const _: () = assert!(CODE_PAD_Y_SZ == 72);
const _: () = assert!((CODE_PAD_Y_HU as i64 * 8) / (HU_PER_POINT as i64) == CODE_PAD_Y_SZ as i64);
/// Letter CSS `hr{border-top:3px}` at 96dpi = 2.25pt = 225 HU.
/// OOXML `w:sz` is eighths of a point (18), not Word's hairline 6/12.
const HR_BORDER_HU: Hu = 225;
/// Letter CSS `hr{margin:1.1rem 0}` = 1320 HU = 264 twips. Word 0pt is jammed.
const HR_BEFORE_HU: Hu = 1320;
const HR_AFTER_HU: Hu = 1320;
const _: () = assert!(HR_BORDER_HU == 225);
const _: () = assert!(HR_BEFORE_HU == 1320);
const _: () = assert!(HR_AFTER_HU == 1320);
/// Letter CSS `code{background:#ccfbf1;color:#134e4a}`.
const CODE_FILL: &str = "CCFBF1";
const CODE_COLOR: &str = "134E4A";
/// Letter CSS `pre,code{font-family:ui-monospace,Consolas,monospace}`.
const CODE_FONT: &str = "Consolas";
/// Letter CSS `code{padding:.1em .35em}` at 10pt ≈ 3.5pt. OOXML rPr `w:bdr` space is points.
const CODE_BDR_SPACE_PT: i32 = 4;
/// Letter CSS `code{font-size:.92em}` of Word 11pt (sz 22) is 10pt (sz 20).
const CODE_SPAN_SZ: i32 = 20;
/// Letter CSS `mark{background:#fde68a}`. Word highlighter palette is yellow; shading keeps amber.
const MARK_FILL: &str = "FDE68A";
/// Letter CSS `mark{color:#134e4a}`. Amber fill without teal type is Word
/// highlighter black-on-yellow, not WeasyPrint letter input (python-docx / Pandoc).
const MARK_COLOR: &str = BODY_COLOR;
/// Letter CSS `mark{padding:.05em .2em}` at 10pt ≈ 2pt.
const MARK_BDR_SPACE_PT: i32 = 2;
const _: () = assert!(CODE_BDR_SPACE_PT == 4);
const _: () = assert!(CODE_SPAN_SZ == 20);
const _: () = assert!(CODE_FONT.as_bytes()[0] == b'C');
const _: () = assert!(MARK_BDR_SPACE_PT == 2);
const _: () = assert!(MARK_COLOR.as_bytes()[0] == b'1' && MARK_COLOR.as_bytes()[5] == b'A');
/// Word 2010 checkbox empty box (U+2610). ASCII `[ ]` is not a box in Word.
const TASK_UNCHECKED_GLYPH: char = '\u{2610}';
/// Word 2010 checkbox checked box (U+2612). `w14:checkedState` default.
const TASK_CHECKED_GLYPH: char = '\u{2612}';
/// `w:numId` for `- [ ]`. Not the disc list (`1`) and not decimal (`2`).
const TASK_UNCHECKED_NUM_ID: u32 = 3;
/// `w:numId` for `- [x]`.
const TASK_CHECKED_NUM_ID: u32 = 4;
/// Letter CSS `ul.tasks input{width:.9em;height:.9em;border:1.5pt solid #134e4a}`.
/// .9em at 10pt plus 1.5pt×2 border is 12pt (`w:sz` 24). Missing sz inherits a
/// tofu-scale glyph, not a visible Word checkbox (python-docx / Pandoc).
const TASK_BOX_SZ: i32 = 24;
const _: () = assert!(TASK_UNCHECKED_GLYPH == '\u{2610}');
const _: () = assert!(TASK_CHECKED_GLYPH == '\u{2612}');
const _: () = assert!(TASK_UNCHECKED_NUM_ID == 3);
const _: () = assert!(TASK_CHECKED_NUM_ID == 4);
const _: () = assert!(TASK_BOX_SZ == 24);
/// Letter CSS `th{background:#ccfbf1}`. Same wash as inline `code`.
const TH_FILL: &str = "CCFBF1";
/// Letter CSS `th{color:#134e4a}`.
const TH_COLOR: &str = "134E4A";
/// Letter CSS `th{border-bottom:2px solid #0d9488}` at 96dpi: 2px = 150 HU.
const TH_RULE_HU: Hu = 2 * (HU_PER_INCH / 96);
/// Letter CSS `th{border-bottom:2px solid #0d9488}`.
const TH_RULE_COLOR: &str = "0D9488";
const _: () = assert!(TH_RULE_HU == 150);
/// Letter CSS `th,td{border:1px solid #cbd5e1}` at 96dpi = 75 HU. OOXML sz 6.
/// Word TableGrid `000000` sz 4 is a black hairline, not WeasyPrint letter input.
const TBL_BORDER_HU: Hu = HU_PER_INCH / 96;
const TBL_BORDER_COLOR: &str = "CBD5E1";
const _: () = assert!(TBL_BORDER_HU == 75);
/// Letter CSS `th,td{font-size:.95rem}` at 16px rem / 96dpi: 0.95rem = 11.4pt.
/// OOXML `w:sz` is half-points; 23 = 11.5pt (closest). Body 11pt / code 10pt is
/// the wrong hop-table type next to WeasyPrint (python-docx / Pandoc inherit Normal).
const CELL_SZ: i32 = 23;
const _: () = assert!(CELL_SZ == 23);
/// Letter CSS `th,td{vertical-align:top}`. Word TableGrid / python-docx omit
/// `w:vAlign`; wrapped hop cells inherit center and punch sparse bands vs
/// WeasyPrint letter input (`thead,tbody{vertical-align:middle}`).
const CELL_VALIGN: &str = "top";
const _: () = assert!(CELL_VALIGN.as_bytes()[0] == b't' && CELL_VALIGN.as_bytes()[2] == b'p');
/// Letter CSS `@page{size:A4;margin:2.5rem 2.75rem}` left/right at 16px rem /
/// 96dpi: 2.75rem = 3300 HU. Hangul [`DEFAULT_MARGIN_HU`] 8504 (~30mm) and
/// WeasyPrint UA `@page{margin:75px}` (5625 HU) are a sparse Word frame
/// (python-docx / Pandoc keep 1.18in).
const PAGE_MARGIN_X_HU: Hu = 3300;
/// Letter CSS `@page` top/bottom 2.5rem = 3000 HU.
const PAGE_MARGIN_Y_HU: Hu = 3000;
const _: () = assert!(PAGE_MARGIN_X_HU == 3300);
const _: () = assert!(PAGE_MARGIN_Y_HU == 3000);
const _: () = assert!(PAGE_MARGIN_X_HU == 44 * (HU_PER_INCH / 96));
const _: () = assert!(PAGE_MARGIN_Y_HU == 40 * (HU_PER_INCH / 96));
const _: () = assert!(PAGE_MARGIN_X_HU > PAGE_MARGIN_Y_HU);
const _: () = assert!(PAGE_MARGIN_X_HU < DEFAULT_MARGIN_HU);
const _: () = assert!(PAGE_MARGIN_Y_HU < DEFAULT_MARGIN_HU);
const _: () = assert!(PAGE_MARGIN_X_HU != 75 * (HU_PER_INCH / 96));
const _: () = assert!(PAGE_MARGIN_Y_HU != 75 * (HU_PER_INCH / 96));

fn hu_to_emu(hu: Hu) -> i64 {
    i64::from(hu.max(1)).saturating_mul(EMU_PER_HU)
}

/// Raise `width`×`height` so both edges are at least [`MIN_IMAGE_EDGE_HU`].
/// Zero/missing size becomes a 24pt square so `a:blip` is visible in Word.
fn visible_image_size(width: Hu, height: Hu) -> (Hu, Hu) {
    let w = width.max(0);
    let h = height.max(0);
    let short = match (w == 0, h == 0) {
        (true, true) => 0,
        (true, false) => h,
        (false, true) => w,
        (false, false) => w.min(h),
    };
    if short >= MIN_IMAGE_EDGE_HU {
        return (w.max(1), h.max(1));
    }
    if short <= 0 {
        return (MIN_IMAGE_EDGE_HU, MIN_IMAGE_EDGE_HU);
    }
    let scale_num = i64::from(MIN_IMAGE_EDGE_HU);
    let scale_den = i64::from(short);
    let nw = (i64::from(w) * scale_num / scale_den).max(1);
    let nh = (i64::from(h) * scale_num / scale_den).max(1);
    (
        i32::try_from(nw).unwrap_or(i32::MAX),
        i32::try_from(nh).unwrap_or(i32::MAX),
    )
}

fn emu_to_hu(emu: i64) -> Hu {
    emu.saturating_div(EMU_PER_HU)
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as Hu
}

pub fn read(bytes: &[u8]) -> Result<Document, Error> {
    if bytes.len() < 4 || bytes[0..2] != [0x50, 0x4B] {
        return Err(Error::NotDocx);
    }
    let mut zip = ZipArchive::new(Cursor::new(bytes.to_vec()))?;
    let mut rels = HashMap::new();
    if let Ok(mut f) = zip.by_name("word/_rels/document.xml.rels") {
        let mut rels_xml = String::new();
        f.read_to_string(&mut rels_xml)?;
        rels = parse_rels(&rels_xml);
    }
    let mut xml = String::new();
    {
        let mut f = zip
            .by_name("word/document.xml")
            .map_err(|_| Error::NotDocx)?;
        f.read_to_string(&mut xml)?;
    }
    let mut styles = HashMap::new();
    if let Ok(mut f) = zip.by_name("word/styles.xml") {
        let mut styles_xml = String::new();
        f.read_to_string(&mut styles_xml)?;
        styles = parse_styles_num(&styles_xml);
    }
    let mut media = HashMap::new();
    for (id, rel) in &rels {
        if !rel_is_image(&rel.ty) {
            continue;
        }
        let path = resolve_word_target(&rel.target);
        if let Ok(mut f) = zip.by_name(&path) {
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)?;
            let mime = mime_of(&rel.target, &buf);
            media.insert(id.clone(), MediaPart { bytes: buf, mime });
        }
    }
    parse_document_xml(&xml, &rels, &media, &styles)
}

fn rel_is_image(ty: &str) -> bool {
    ty.rsplit('/').next().is_some_and(|s| s.eq_ignore_ascii_case("image"))
}

fn resolve_word_target(target: &str) -> String {
    let t = target.replace('\\', "/");
    let t = t.split(['?', '#']).next().unwrap_or(&t);
    if let Some(rest) = t.strip_prefix('/') {
        rest.to_string()
    } else if t.starts_with("word/") {
        t.to_string()
    } else {
        format!("word/{t}")
    }
}

fn mime_of(name: &str, bytes: &[u8]) -> String {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".png") || bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return "image/png".into();
    }
    if lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || bytes.starts_with(&[0xFF, 0xD8, 0xFF])
    {
        return "image/jpeg".into();
    }
    if lower.ends_with(".gif") || bytes.starts_with(b"GIF8") {
        return "image/gif".into();
    }
    if lower.ends_with(".bmp") || bytes.starts_with(b"BM") {
        return "image/bmp".into();
    }
    if lower.ends_with(".tif") || lower.ends_with(".tiff") {
        return "image/tiff".into();
    }
    if lower.ends_with(".emf") {
        return "image/x-emf".into();
    }
    if lower.ends_with(".wmf") {
        return "image/x-wmf".into();
    }
    if lower.ends_with(".svg") {
        return "image/svg+xml".into();
    }
    if lower.ends_with(".webp") {
        return "image/webp".into();
    }
    "image/png".into()
}

struct StyleNum {
    num_id: Option<u32>,
    ilvl: Option<u8>,
    based_on: Option<String>,
}

fn parse_styles_num(xml: &str) -> HashMap<String, StyleNum> {
    let mut map = HashMap::new();
    if xml.is_empty() {
        return map;
    }
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut in_style = false;
    let mut in_ppr = false;
    let mut style_id = String::new();
    let mut based_on = None;
    let mut num_id = None;
    let mut ilvl = None;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e) | Event::Empty(e)) => {
                let name = local_name(&e);
                match name.as_str() {
                    "style" => {
                        in_style = true;
                        in_ppr = false;
                        style_id = attr(&e, "styleId").unwrap_or_default();
                        based_on = None;
                        num_id = None;
                        ilvl = None;
                    }
                    "basedOn" if in_style => based_on = attr(&e, "val"),
                    "pPr" if in_style => in_ppr = true,
                    "ilvl" if in_style && in_ppr => {
                        ilvl = attr(&e, "val").and_then(|v| v.parse().ok());
                    }
                    "numId" if in_style && in_ppr => {
                        num_id = attr(&e, "val").and_then(|v| v.parse().ok());
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                match name.as_str() {
                    "pPr" => in_ppr = false,
                    "style" => {
                        if in_style && !style_id.is_empty() {
                            map.insert(
                                style_id.clone(),
                                StyleNum {
                                    num_id,
                                    ilvl,
                                    based_on: based_on.take(),
                                },
                            );
                        }
                        in_style = false;
                        in_ppr = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    map
}

fn style_numbering(styles: &HashMap<String, StyleNum>, style_id: &str) -> (Option<u32>, u8) {
    let mut current = Some(style_id.to_string());
    let mut seen = BTreeSet::new();
    while let Some(id) = current {
        if !seen.insert(id.clone()) {
            break;
        }
        let Some(s) = styles.get(&id) else {
            break;
        };
        if let Some(n) = s.num_id {
            return (Some(n), s.ilvl.unwrap_or(0));
        }
        current = s.based_on.clone();
    }
    (None, 0)
}

fn parse_rels(xml: &str) -> HashMap<String, Rel> {
    let mut map = HashMap::new();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e) | Event::Empty(e)) => {
                if local_name(&e) == "Relationship" {
                    let id = attr(&e, "Id").or_else(|| attr(&e, "id"));
                    let target = attr(&e, "Target").or_else(|| attr(&e, "target"));
                    let ty = attr(&e, "Type").or_else(|| attr(&e, "type")).unwrap_or_default();
                    if let (Some(id), Some(target)) = (id, target) {
                        map.insert(id, Rel { target, ty });
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    map
}

pub fn write(doc: &Document) -> Result<Vec<u8>, Error> {
    let images = collect_images(doc);
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut cursor);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file("[Content_Types].xml", deflated)?;
        zip.write_all(content_types_xml(&images).as_bytes())?;
        zip.start_file("_rels/.rels", deflated)?;
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        )?;
        zip.start_file("word/_rels/document.xml.rels", deflated)?;
        zip.write_all(document_rels(doc).as_bytes())?;
        zip.start_file("word/styles.xml", deflated)?;
        zip.write_all(STYLES_XML.as_bytes())?;
        zip.start_file("word/settings.xml", deflated)?;
        zip.write_all(SETTINGS_XML.as_bytes())?;
        zip.start_file("word/numbering.xml", deflated)?;
        zip.write_all(numbering_xml().as_bytes())?;
        zip.start_file("word/document.xml", deflated)?;
        zip.write_all(document_xml(doc).as_bytes())?;
        for (idx, img) in images.iter().enumerate() {
            let ext = ext_from_image(img);
            zip.start_file(format!("word/media/image{}.{ext}", idx + 1), deflated)?;
            zip.write_all(&img.bytes)?;
        }
        zip.finish()?;
    }
    Ok(cursor.into_inner())
}

fn content_types_xml(images: &[&ImageData]) -> String {
    let mut s = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>"#,
    );
    let mut extras = BTreeSet::new();
    for img in images {
        extras.insert((
            ext_from_image(img).to_string(),
            content_type_for_image(img).to_string(),
        ));
    }
    for (ext, ct) in extras {
        s.push_str(&format!(
            r#"<Default Extension="{ext}" ContentType="{ct}"/>"#
        ));
    }
    s.push_str(
        r#"<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/></Types>"#,
    );
    s
}

fn jpeg_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8, 0xFF])
}

fn png_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x89, b'P', b'N', b'G'])
}

fn is_jpeg(img: &ImageData) -> bool {
    img.mime.eq_ignore_ascii_case("image/jpeg")
        || img.mime.eq_ignore_ascii_case("image/jpg")
        || jpeg_magic(&img.bytes)
}

fn ext_from_image(img: &ImageData) -> &'static str {
    if png_magic(&img.bytes) {
        "png"
    } else if is_jpeg(img) {
        "jpeg"
    } else {
        ext_from_mime(&img.mime)
    }
}

fn content_type_for_image(img: &ImageData) -> &str {
    if png_magic(&img.bytes) {
        "image/png"
    } else if is_jpeg(img) {
        "image/jpeg"
    } else {
        content_type_for_mime(&img.mime)
    }
}

fn ext_from_mime(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" | "image/jpg" => "jpeg",
        "image/gif" => "gif",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        "image/svg+xml" => "svg",
        "image/x-emf" | "image/emf" => "emf",
        "image/x-wmf" | "image/wmf" => "wmf",
        "image/webp" => "webp",
        _ => "png",
    }
}

fn content_type_for_mime(mime: &str) -> &str {
    if mime.starts_with("image/") && mime != "image/jpg" {
        mime
    } else if mime == "image/jpg" {
        "image/jpeg"
    } else {
        "image/png"
    }
}

fn document_rels(doc: &Document) -> String {
    let mut s = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>"#,
    );
    let mut i = 3u32;
    for img in collect_images(doc) {
        let ext = ext_from_image(img);
        let n = i - 2;
        s.push_str(&format!(
            r#"<Relationship Id="rId{i}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image{n}.{ext}"/>"#
        ));
        i += 1;
    }
    for target in hyperlink_targets(doc) {
        s.push_str(&format!(
            r#"<Relationship Id="rId{i}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="{}" TargetMode="External"/>"#,
            xml_escape(&target)
        ));
        i += 1;
    }
    // After images/hyperlinks so letter a:blip stays rId3 (python-docx/Pandoc).
    s.push_str(&format!(
        r#"<Relationship Id="rId{i}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/>"#
    ));
    s.push_str("</Relationships>");
    s
}

fn hyperlink_targets(doc: &Document) -> Vec<String> {
    let mut out = Vec::new();
    for section in &doc.sections {
        collect_hyperlinks(&section.body, &mut out);
    }
    out
}

fn collect_hyperlinks(blocks: &[Block], out: &mut Vec<String>) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => {
                for run in &p.runs {
                    if let RunContent::Inline(InlineObject::Hyperlink { target, display }) =
                        &run.content
                        && !display.is_empty()
                    {
                        out.push(target.clone());
                    }
                }
            }
            Block::Table(t) => {
                for row in &t.rows {
                    for cell in &row.cells {
                        collect_hyperlinks(&cell.blocks, out);
                    }
                }
            }
            Block::Float(_) | Block::Break(_) => {}
        }
    }
}

fn collect_images(doc: &Document) -> Vec<&ImageData> {
    let mut out = Vec::new();
    for section in &doc.sections {
        collect_images_in_blocks(&section.body, &mut out);
    }
    out
}

fn collect_images_in_blocks<'a>(blocks: &'a [Block], out: &mut Vec<&'a ImageData>) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => {
                for run in &p.runs {
                    if let RunContent::Inline(InlineObject::Image(img)) = &run.content {
                        out.push(img);
                    }
                }
            }
            Block::Table(t) => {
                for row in &t.rows {
                    for cell in &row.cells {
                        collect_images_in_blocks(&cell.blocks, out);
                    }
                }
            }
            Block::Float(Float::Image(img)) => out.push(img),
            Block::Float(_) | Block::Break(_) => {}
        }
    }
}

struct WriteCtx {
    image_i: u32,
    link_i: u32,
    image_base: u32,
    link_base: u32,
}

impl WriteCtx {
    fn new(n_images: u32) -> Self {
        Self {
            image_i: 0,
            link_i: 0,
            image_base: 3,
            link_base: 3 + n_images,
        }
    }

    fn next_image_rid(&mut self) -> u32 {
        let rid = self.image_base + self.image_i;
        self.image_i += 1;
        rid
    }

    fn next_link_rid(&mut self) -> u32 {
        let rid = self.link_base + self.link_i;
        self.link_i += 1;
        rid
    }
}

/// Painted left/right page inset. Letter CSS `@page` 2.75rem, not Hangul 30mm.
fn page_margin_x(ir: Hu) -> Hu {
    if ir == DEFAULT_MARGIN_HU {
        PAGE_MARGIN_X_HU
    } else {
        ir.max(0)
    }
}

/// Painted top/bottom page inset. Letter CSS `@page` 2.5rem, not Hangul 30mm.
fn page_margin_y(ir: Hu) -> Hu {
    if ir == DEFAULT_MARGIN_HU {
        PAGE_MARGIN_Y_HU
    } else {
        ir.max(0)
    }
}

/// Header/footer distance sits inside the painted margin. Hangul
/// `DEFAULT_MARGIN_HU / 2` (4252) is past letter 2.5rem (3000) — Word clips.
fn page_band_distance(ir: Hu, margin_ir: Hu, margin_out: Hu) -> Hu {
    if margin_ir == DEFAULT_MARGIN_HU && ir == DEFAULT_MARGIN_HU / 2 {
        margin_out / 2
    } else {
        ir.max(0)
    }
}

/// Table `w:tblW` / grid stretch to the painted measure, not Hangul 30mm gutters.
fn letter_content_width(page: &PageSetup) -> Hu {
    (page.width
        - page_margin_x(page.margin_left)
        - page_margin_x(page.margin_right)
        - page.gutter)
        .max(1)
}

fn document_xml(doc: &Document) -> String {
    let mut s = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><w:body>"#,
    );
    let sections = if doc.sections.is_empty() {
        vec![Section::default()]
    } else {
        doc.sections.clone()
    };
    let mut ctx = WriteCtx::new(collect_images(doc).len() as u32);
    for (si, section) in sections.iter().enumerate() {
        if section.body.is_empty() {
            s.push_str("<w:p><w:r><w:t/></w:r></w:p>");
        } else {
            for (bi, block) in section.body.iter().enumerate() {
                match block {
                    Block::Paragraph(p) => {
                        s.push_str(&p_xml(p, &mut ctx, false, section.body.get(bi + 1), false))
                    }
                    Block::Table(t) => {
                        s.push_str(&tbl_xml(t, &mut ctx, letter_content_width(&section.page)))
                    }
                    Block::Break(BreakKind::Thematic) => s.push_str(&thematic_break_xml()),
                    Block::Float(Float::Image(img)) => {
                        s.push_str(&float_image_p_xml(img, &mut ctx))
                    }
                    Block::Float(_) | Block::Break(_) => s.push_str("<w:p/>"),
                }
            }
        }
        s.push_str("<w:sectPr>");
        s.push_str(&format!(
            r#"<w:pgSz w:w="{}" w:h="{}" w:orient="{}"/>"#,
            hu_to_twips(section.page.width),
            hu_to_twips(section.page.height),
            if section.page.landscape {
                "landscape"
            } else {
                "portrait"
            }
        ));
        let margin_top = page_margin_y(section.page.margin_top);
        let margin_bottom = page_margin_y(section.page.margin_bottom);
        let margin_left = page_margin_x(section.page.margin_left);
        let margin_right = page_margin_x(section.page.margin_right);
        s.push_str(&format!(
            r#"<w:pgMar w:top="{}" w:right="{}" w:bottom="{}" w:left="{}" w:header="{}" w:footer="{}" w:gutter="{}"/>"#,
            hu_to_twips(margin_top),
            hu_to_twips(margin_right),
            hu_to_twips(margin_bottom),
            hu_to_twips(margin_left),
            hu_to_twips(page_band_distance(
                section.page.header_distance,
                section.page.margin_top,
                margin_top
            )),
            hu_to_twips(page_band_distance(
                section.page.footer_distance,
                section.page.margin_bottom,
                margin_bottom
            )),
            hu_to_twips(section.page.gutter)
        ));
        s.push_str("</w:sectPr>");
        let _ = si;
    }
    s.push_str("</w:body></w:document>");
    s
}

/// python-docx / Pandoc `word/settings.xml`. Missing this opens as Word 2003
/// layout: compressed H1 tracking, table style sz ignored, 2px hop gutters.
const SETTINGS_XML: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
    r#"<w:zoom w:percent="100"/>"#,
    r#"<w:displayBackgroundShape/>"#,
    r#"<w:defaultTabStop w:val="720"/>"#,
    r#"<w:characterSpacingControl w:val="doNotCompress"/>"#,
    r#"<w:compat>"#,
    r#"<w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/>"#,
    r#"<w:compatSetting w:name="overrideTableStyleFontSizeAndJustification" w:uri="http://schemas.microsoft.com/office/word" w:val="1"/>"#,
    r#"</w:compat></w:settings>"#,
);

const STYLES_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Segoe UI" w:hAnsi="Segoe UI" w:cs="Segoe UI"/><w:color w:val="134E4A"/><w:sz w:val="22"/><w:szCs w:val="22"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="168" w:line="276" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/><w:pPr><w:spacing w:after="168" w:line="276" w:lineRule="auto"/></w:pPr><w:rPr><w:rFonts w:ascii="Segoe UI" w:hAnsi="Segoe UI" w:cs="Segoe UI"/><w:color w:val="134E4A"/><w:sz w:val="22"/><w:szCs w:val="22"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:pBdr><w:bottom w:val="single" w:sz="18" w:space="5" w:color="134E4A"/></w:pBdr><w:spacing w:before="240" w:after="160" w:line="288" w:lineRule="auto"/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:rFonts w:ascii="Segoe UI" w:hAnsi="Segoe UI" w:cs="Segoe UI"/><w:b/><w:color w:val="134E4A"/><w:spacing w:val="-6"/><w:sz w:val="32"/><w:szCs w:val="32"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:pBdr><w:bottom w:val="single" w:sz="12" w:space="2" w:color="0D9488"/></w:pBdr><w:spacing w:before="200" w:after="80" w:line="288" w:lineRule="auto"/><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:rFonts w:ascii="Segoe UI" w:hAnsi="Segoe UI" w:cs="Segoe UI"/><w:b/><w:color w:val="134E4A"/><w:sz w:val="26"/><w:szCs w:val="26"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Heading3"><w:name w:val="heading 3"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before="80" w:after="80" w:line="288" w:lineRule="auto"/><w:outlineLvl w:val="2"/></w:pPr><w:rPr><w:rFonts w:ascii="Segoe UI" w:hAnsi="Segoe UI" w:cs="Segoe UI"/><w:b/><w:color w:val="134E4A"/><w:sz w:val="24"/><w:szCs w:val="24"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr><w:pBdr><w:left w:val="single" w:sz="24" w:space="4" w:color="134E4A"/></w:pBdr><w:spacing w:before="120" w:after="240"/><w:ind w:left="720"/></w:pPr><w:rPr><w:rFonts w:ascii="Segoe UI" w:hAnsi="Segoe UI" w:cs="Segoe UI"/><w:color w:val="134E4A"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="ListParagraph"><w:name w:val="List Paragraph"/><w:basedOn w:val="Normal"/><w:uiPriority w:val="34"/><w:qFormat/><w:pPr><w:ind w:left="300"/><w:spacing w:after="84"/></w:pPr><w:rPr><w:rFonts w:ascii="Segoe UI" w:hAnsi="Segoe UI" w:cs="Segoe UI"/></w:rPr></w:style><w:style w:type="character" w:styleId="Code"><w:name w:val="Code"/><w:rPr><w:rFonts w:ascii="Consolas" w:hAnsi="Consolas"/><w:color w:val="134E4A"/><w:sz w:val="20"/><w:szCs w:val="20"/><w:bdr w:val="single" w:sz="4" w:space="4" w:color="CCFBF1"/><w:shd w:val="clear" w:fill="CCFBF1"/></w:rPr></w:style><w:style w:type="character" w:styleId="Mark"><w:name w:val="Mark"/><w:rPr><w:rFonts w:ascii="Segoe UI" w:hAnsi="Segoe UI" w:cs="Segoe UI"/><w:color w:val="134E4A"/><w:highlight w:val="yellow"/><w:bdr w:val="single" w:sz="4" w:space="2" w:color="FDE68A"/><w:shd w:val="clear" w:fill="FDE68A"/></w:rPr></w:style><w:style w:type="character" w:styleId="Strike"><w:name w:val="Strikethrough"/><w:rPr><w:strike/></w:rPr></w:style><w:style w:type="character" w:styleId="Underline"><w:name w:val="Underline"/><w:rPr><w:u w:val="single"/></w:rPr></w:style><w:style w:type="character" w:styleId="Superscript"><w:name w:val="Superscript"/><w:rPr><w:vertAlign w:val="superscript"/></w:rPr></w:style><w:style w:type="character" w:styleId="Subscript"><w:name w:val="Subscript"/><w:rPr><w:vertAlign w:val="subscript"/></w:rPr></w:style><w:style w:type="character" w:styleId="Hyperlink"><w:name w:val="Hyperlink"/><w:rPr><w:rFonts w:ascii="Segoe UI" w:hAnsi="Segoe UI" w:cs="Segoe UI"/><w:color w:val="0D9488"/><w:u w:val="single"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="HorizontalLine"><w:name w:val="Horizontal Line"/><w:basedOn w:val="Normal"/><w:pPr><w:pBdr><w:bottom w:val="single" w:sz="18" w:space="1" w:color="134E4A"/></w:pBdr><w:spacing w:before="264" w:after="264"/></w:pPr></w:style><w:style w:type="paragraph" w:styleId="CodeBlock"><w:name w:val="Code Block"/><w:basedOn w:val="Normal"/><w:pPr><w:pBdr><w:top w:val="single" w:sz="72" w:space="0" w:color="CCFBF1"/><w:bottom w:val="single" w:sz="72" w:space="0" w:color="CCFBF1"/></w:pBdr><w:spacing w:before="144" w:after="240"/><w:ind w:left="240" w:right="240"/><w:shd w:val="clear" w:fill="CCFBF1"/></w:pPr><w:rPr><w:rFonts w:ascii="Consolas" w:hAnsi="Consolas"/></w:rPr></w:style><w:style w:type="table" w:styleId="TableGrid"><w:name w:val="Table Grid"/><w:tblPr><w:tblCellSpacing w:w="0" w:type="dxa"/><w:tblBorders><w:top w:val="single" w:sz="6" w:space="0" w:color="CBD5E1"/><w:left w:val="single" w:sz="6" w:space="0" w:color="CBD5E1"/><w:bottom w:val="single" w:sz="6" w:space="0" w:color="CBD5E1"/><w:right w:val="single" w:sz="6" w:space="0" w:color="CBD5E1"/><w:insideH w:val="single" w:sz="6" w:space="0" w:color="CBD5E1"/><w:insideV w:val="single" w:sz="6" w:space="0" w:color="CBD5E1"/></w:tblBorders></w:tblPr><w:tcPr><w:vAlign w:val="top"/></w:tcPr><w:rPr><w:sz w:val="23"/><w:szCs w:val="23"/></w:rPr><w:tblStylePr w:type="firstRow"><w:tcPr><w:tcBorders><w:bottom w:val="single" w:sz="12" w:space="0" w:color="0D9488"/></w:tcBorders><w:shd w:val="clear" w:fill="CCFBF1"/><w:vAlign w:val="top"/></w:tcPr><w:rPr><w:b/><w:color w:val="134E4A"/><w:sz w:val="23"/><w:szCs w:val="23"/></w:rPr></w:tblStylePr></w:style></w:styles>"#;

fn hu_to_half_points(hu: i32) -> i32 {
    (hu / 50).max(1)
}

fn half_points_to_hu(hp: i32) -> i32 {
    hp.saturating_mul(50)
}

/// Direct rFonts so Word paints letter Segoe UI even if Normal is ignored.
fn body_rfonts_xml() -> String {
    format!(
        r#"<w:rFonts w:ascii="{BODY_FONT}" w:hAnsi="{BODY_FONT}" w:cs="{BODY_FONT}"/>"#
    )
}

/// Direct rFonts so Word paints Consolas on `pre` even if CodeBlock is ignored.
fn code_block_rfonts_xml() -> String {
    format!(r#"<w:rFonts w:ascii="{CODE_FONT}" w:hAnsi="{CODE_FONT}"/>"#)
}

/// Direct w:sz so Word paints letter `th,td` 0.95rem even if TableGrid is ignored.
fn cell_sz_xml() -> String {
    format!(r#"<w:sz w:val="{CELL_SZ}"/><w:szCs w:val="{CELL_SZ}"/>"#)
}

/// Direct w:vAlign so Word tops hop cells even if TableGrid is ignored.
fn cell_valign_xml() -> String {
    format!(r#"<w:vAlign w:val="{CELL_VALIGN}"/>"#)
}

/// Direct rPr so Word paints the Consolas chip even if the Code character style is ignored.
/// `w:sz` 20 is letter `.92em` of Word 11pt — style-only Code is body 11pt, not the chip.
fn code_rpr_xml() -> String {
    format!(
        r#"<w:rStyle w:val="Code"/><w:rFonts w:ascii="{CODE_FONT}" w:hAnsi="{CODE_FONT}"/><w:color w:val="{CODE_COLOR}"/><w:sz w:val="{CODE_SPAN_SZ}"/><w:szCs w:val="{CODE_SPAN_SZ}"/><w:bdr w:val="single" w:sz="4" w:space="{CODE_BDR_SPACE_PT}" w:color="{CODE_FILL}"/><w:shd w:val="clear" w:fill="{CODE_FILL}"/>"#
    )
}

/// Direct rPr: Word highlighter plus letter amber shading/padding and letter
/// teal type so `==mark==` is not highlighter black-on-yellow.
fn mark_rpr_xml() -> String {
    format!(
        r#"<w:rStyle w:val="Mark"/><w:color w:val="{MARK_COLOR}"/><w:highlight w:val="yellow"/><w:bdr w:val="single" w:sz="4" w:space="{MARK_BDR_SPACE_PT}" w:color="{MARK_FILL}"/><w:shd w:val="clear" w:fill="{MARK_FILL}"/>"#
    )
}

/// python-docx / Word Font Effects Single. Style-only Strike is a missing line.
fn strike_rstyle_xml() -> &'static str {
    r#"<w:rStyle w:val="Strike"/>"#
}

/// python-docx / Word Font Effects Single. Style-only Underline is a missing rule.
fn underline_rstyle_xml() -> &'static str {
    r#"<w:rStyle w:val="Underline"/>"#
}

/// python-docx / Word Font Effects Superscript. Style-only Super is missing type.
fn superscript_rstyle_xml() -> &'static str {
    r#"<w:rStyle w:val="Superscript"/>"#
}

/// python-docx / Word Font Effects Subscript. Style-only Sub is missing type.
fn subscript_rstyle_xml() -> &'static str {
    r#"<w:rStyle w:val="Subscript"/>"#
}

/// Letter CSS `blockquote{border-left:4px solid #134e4a}`. Direct pBdr so Word
/// paints the bar even if the Quote style is ignored (python-docx / Pandoc).
fn quote_pbdr_xml() -> String {
    let sz = hu_to_border_sz(QUOTE_BAR_HU);
    format!(
        r#"<w:pBdr><w:left w:val="single" w:sz="{sz}" w:space="{QUOTE_BDR_SPACE_PT}" w:color="{QUOTE_COLOR}"/></w:pBdr>"#
    )
}

/// Letter CSS `h1{letter-spacing:-.02em}`. Direct rPr spacing so Word
/// tracks H1 even if Heading1 is ignored (python-docx / Pandoc). H2 stays 0.
fn heading_spacing_xml(level: u8) -> String {
    if level != 1 {
        return String::new();
    }
    format!(r#"<w:spacing w:val="{HEADING1_SPACING}"/>"#)
}

/// Letter CSS `h1{border-bottom:3px solid #134e4a}` / `h2{border-bottom:2px solid #0d9488}`.
/// Direct pBdr so Word paints the rule even if Heading1/2 is ignored.
fn heading_pbdr_xml(level: u8) -> String {
    let (hu, space, color) = match level {
        1 => (HEADING1_RULE_HU, HEADING1_BDR_SPACE_PT, BODY_COLOR),
        2 => (HEADING2_RULE_HU, HEADING2_BDR_SPACE_PT, TH_RULE_COLOR),
        _ => return String::new(),
    };
    let sz = hu_to_border_sz(hu);
    format!(
        r#"<w:pBdr><w:bottom w:val="single" w:sz="{sz}" w:space="{space}" w:color="{color}"/></w:pBdr>"#
    )
}

/// Letter CSS `pre{padding:.75rem 1rem}`. Direct ind so Word pads the teal
/// wash even if the CodeBlock style is ignored (python-docx / Pandoc).
fn code_block_ind_xml() -> String {
    let pad = hu_to_twips(CODE_PAD_X_HU);
    format!(r#"<w:ind w:left="{pad}" w:right="{pad}"/>"#)
}

/// Letter CSS `pre{padding:.75rem 1rem}` top/bottom. Thick same-fill pBdr so
/// Word paints the wash pad even if CodeBlock is ignored (python-docx / Pandoc).
fn code_block_pbdr_xml() -> String {
    let sz = hu_to_border_sz(CODE_PAD_Y_HU);
    format!(
        r#"<w:pBdr><w:top w:val="single" w:sz="{sz}" w:space="0" w:color="{CODE_FILL}"/><w:bottom w:val="single" w:sz="{sz}" w:space="0" w:color="{CODE_FILL}"/></w:pBdr>"#
    )
}

/// Letter CSS `table{margin:1rem 0}`. Word has no tblPr before/after; the
/// previous paragraph's after is what python-docx/Pandoc leave as the gap.
fn space_after_before_table(after: Hu, in_cell: bool, next: Option<&Block>) -> Hu {
    if !in_cell && matches!(next, Some(Block::Table(_))) {
        after.max(TABLE_MARGIN_HU)
    } else {
        after
    }
}

fn thematic_break_xml() -> String {
    let sz = hu_to_border_sz(HR_BORDER_HU);
    let before = hu_to_twips(HR_BEFORE_HU);
    let after = hu_to_twips(HR_AFTER_HU);
    // Word AutoFormat / python-docx: empty w:p + bottom pBdr. Direct spacing
    // so Normal's 8pt after cannot collapse the letter 1.1rem gap.
    format!(
        r#"<w:p><w:pPr><w:pStyle w:val="HorizontalLine"/><w:pBdr><w:bottom w:val="single" w:sz="{sz}" w:space="1" w:color="134E4A"/></w:pBdr><w:spacing w:before="{before}" w:after="{after}"/></w:pPr><w:r><w:t/></w:r></w:p>"#
    )
}

fn list_ind_xml(ilvl: i32) -> String {
    let left = LIST_LEFT_TWIPS.saturating_mul(ilvl.saturating_add(1));
    format!(r#"<w:pPr><w:ind w:left="{left}" w:hanging="{LIST_HANGING_TWIPS}"/></w:pPr>"#)
}

/// Disc (abstract 0) / decimal (abstract 1). Indent from [`LIST_LEFT_TWIPS`].
fn list_abstract_num(abstract_id: u32, fmt: &str) -> String {
    let mut s = format!(
        r#"<w:abstractNum w:abstractNumId="{abstract_id}"><w:multiLevelType w:val="hybridMultilevel"/>"#
    );
    for ilvl in 0i32..3 {
        let text = if fmt == "decimal" {
            format!("%{}.", ilvl + 1)
        } else {
            "•".into()
        };
        s.push_str(&format!(
            r#"<w:lvl w:ilvl="{ilvl}"><w:start w:val="1"/><w:numFmt w:val="{fmt}"/><w:lvlText w:val="{text}"/><w:lvlJc w:val="left"/>{}</w:lvl>"#,
            list_ind_xml(ilvl)
        ));
    }
    s.push_str("</w:abstractNum>");
    s
}

/// Word checkbox bullets (Insert → Bullets gallery / w14:checkbox glyphs).
/// `Segoe UI Symbol` + `MS Gothic` is how Word stores ☐ / ☒ so they cannot tofu.
fn checkbox_abstract_num(abstract_id: u32, glyph: char) -> String {
    let mut s = format!(
        r#"<w:abstractNum w:abstractNumId="{abstract_id}"><w:multiLevelType w:val="hybridMultilevel"/>"#
    );
    for ilvl in 0i32..3 {
        s.push_str(&format!(
            r#"<w:lvl w:ilvl="{ilvl}"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="{glyph}"/><w:lvlJc w:val="left"/>{}<w:rPr><w:rFonts w:ascii="Segoe UI Symbol" w:hAnsi="Segoe UI Symbol" w:eastAsia="MS Gothic" w:cs="Segoe UI Symbol" w:hint="default"/><w:sz w:val="{TASK_BOX_SZ}"/><w:szCs w:val="{TASK_BOX_SZ}"/><w:color w:val="134E4A"/></w:rPr></w:lvl>"#,
            list_ind_xml(ilvl)
        ));
    }
    s.push_str("</w:abstractNum>");
    s
}

fn numbering_xml() -> String {
    let mut s = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
    );
    s.push_str(&list_abstract_num(0, "bullet"));
    s.push_str(&list_abstract_num(1, "decimal"));
    s.push_str(&checkbox_abstract_num(2, TASK_UNCHECKED_GLYPH));
    s.push_str(&checkbox_abstract_num(3, TASK_CHECKED_GLYPH));
    s.push_str(
        r#"<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num><w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num><w:num w:numId="3"><w:abstractNumId w:val="2"/></w:num><w:num w:numId="4"><w:abstractNumId w:val="3"/></w:num></w:numbering>"#,
    );
    s
}

fn float_image_p_xml(img: &ImageData, ctx: &mut WriteCtx) -> String {
    let rid = ctx.next_image_rid();
    let before = hu_to_twips(IMAGE_BEFORE_HU);
    let after = hu_to_twips(IMAGE_AFTER_HU);
    let mut s = format!(
        r#"<w:p><w:pPr><w:spacing w:before="{before}" w:after="{after}"/></w:pPr>"#
    );
    s.push_str(&drawing_run_xml(img, rid));
    s.push_str("</w:p>");
    s
}

fn drawing_run_xml(img: &ImageData, rid: u32) -> String {
    let (width, height) = visible_image_size(img.width, img.height);
    let cx = hu_to_emu(width);
    let cy = hu_to_emu(height);
    let name = format!("Picture {rid}");
    let descr = img
        .alt_text
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| format!(r#" descr="{}""#, xml_escape(s)))
        .unwrap_or_default();
    // OOXML: wrap type sits between effectExtent and docPr (python-docx / ONLYOFFICE).
    let extent = format!(
        r#"<wp:extent cx="{cx}" cy="{cy}"/><wp:effectExtent l="0" t="0" r="0" b="0"/>"#
    );
    let graphic = format!(
        r#"<wp:docPr id="{rid}" name="{name}"{descr}/><wp:cNvGraphicFramePr><a:graphicFrameLocks xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" noChangeAspect="1"/></wp:cNvGraphicFramePr><a:graphic xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:nvPicPr><pic:cNvPr id="0" name="image{rid}"{descr}/><pic:cNvPicPr><a:picLocks noChangeAspect="1"/></pic:cNvPicPr></pic:nvPicPr><pic:blipFill><a:blip r:embed="rId{rid}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic>"#
    );
    let framed = match img.wrap {
        WrapMode::Inline => format!(
            r#"<wp:inline distT="0" distB="0" distL="0" distR="0">{extent}{graphic}</wp:inline>"#
        ),
        wrap => {
            let behind = if wrap == WrapMode::Behind { "1" } else { "0" };
            let wrap_el = match wrap {
                WrapMode::Square => r#"<wp:wrapSquare wrapText="bothSides"/>"#,
                WrapMode::Tight => r#"<wp:wrapTight wrapText="bothSides"/>"#,
                WrapMode::Through => r#"<wp:wrapThrough wrapText="bothSides"/>"#,
                WrapMode::TopAndBottom => r#"<wp:wrapTopAndBottom/>"#,
                WrapMode::Behind | WrapMode::InFront => r#"<wp:wrapNone/>"#,
                WrapMode::Inline => unreachable!(),
            };
            format!(
                r#"<wp:anchor distT="0" distB="0" distL="0" distR="0" simplePos="0" relativeHeight="0" behindDoc="{behind}" locked="0" layoutInCell="1" allowOverlap="1"><wp:simplePos x="0" y="0"/><wp:positionH relativeFrom="column"><wp:posOffset>0</wp:posOffset></wp:positionH><wp:positionV relativeFrom="paragraph"><wp:posOffset>0</wp:posOffset></wp:positionV>{extent}{wrap_el}{graphic}</wp:anchor>"#
            )
        }
    };
    format!(r#"<w:r><w:drawing>{framed}</w:drawing></w:r>"#)
}

fn paragraph_is_image_only(p: &Paragraph) -> bool {
    let mut saw_image = false;
    for run in &p.runs {
        match &run.content {
            RunContent::Inline(InlineObject::Image(_)) => saw_image = true,
            _ => {
                if run.display_text().chars().any(|c| !c.is_whitespace()) {
                    return false;
                }
            }
        }
    }
    saw_image
}

fn heading_style_space(level: u8) -> (Hu, Hu) {
    match level {
        1 => (HEADING1_BEFORE_HU, HEADING1_AFTER_HU),
        2 => (HEADING2_BEFORE_HU, HEADING2_AFTER_HU),
        _ => (HEADING3_BEFORE_HU, HEADING3_AFTER_HU),
    }
}

/// Word / python-docx / Pandoc heading type. Direct `w:sz` on the run would
/// override Heading1 16pt / Heading2 13pt with markdown CSS 21pt / 13.5pt.
fn heading_style_size_hu(level: u8) -> Hu {
    match level {
        1 => HEADING1_SIZE_HU,
        2 => HEADING2_SIZE_HU,
        _ => HEADING3_SIZE_HU,
    }
}

fn paragraph_space_before(p: &Paragraph) -> Hu {
    let before = p.space_before.max(0);
    if paragraph_is_image_only(p) {
        // Letter `img{margin:.6rem 0}` top. IR 80 / Word 0 is jammed under H2.
        return IMAGE_BEFORE_HU;
    }
    if let Some(level) = p.outline_level.filter(|l| *l > 0) {
        before.max(heading_style_space(level).0)
    } else if p.quote {
        before.max(QUOTE_BEFORE_HU)
    } else if p.code_block {
        // Letter `pre{margin:.6rem 0 1rem}` top. IR 80 / Word 6pt is jammed under the quote.
        before.max(CODE_BEFORE_HU)
    } else {
        before
    }
}

fn list_next_level(next: Option<&Block>) -> Option<u8> {
    match next {
        Some(Block::Paragraph(q)) => q.numbering.as_ref().map(|n| n.level),
        _ => None,
    }
}

fn paragraph_space_after(p: &Paragraph, in_cell: bool, next: Option<&Block>) -> Hu {
    let after = p.space_after.max(0);
    if paragraph_is_image_only(p) {
        // Collapsed `p` 0.7rem + `img` 0.6rem. IR 200 / Normal 8pt is jammed.
        return space_after_before_table(IMAGE_AFTER_HU, in_cell, next);
    }
    if let Some(level) = p.outline_level.filter(|l| *l > 0) {
        return space_after_before_table(after.max(heading_style_space(level).1), in_cell, next);
    }
    if in_cell {
        return after;
    }
    if let Some(cur) = p.numbering.as_ref() {
        // Nested `ul ul{margin:.25rem 0}` is tighter than sibling `li` 0.35rem.
        return match list_next_level(next) {
            Some(next_level) if next_level > cur.level => LIST_NESTED_GAP_HU,
            Some(_) => LIST_ITEM_AFTER_HU,
            None => space_after_before_table(after.max(LIST_AFTER_HU), false, next),
        };
    }
    if p.quote {
        return space_after_before_table(after.max(QUOTE_AFTER_HU), false, next);
    }
    if p.code_block {
        // Letter `pre{margin:.6rem 0 1rem}` bottom. IR 200 / Normal 8pt is jammed.
        return space_after_before_table(after.max(CODE_AFTER_HU), false, next);
    }
    // Letter `p{margin:0 0 .7rem}`. IR 200 / Normal 8pt is jammed.
    space_after_before_table(after.max(BODY_AFTER_HU), false, next)
}

fn paragraph_line(p: &Paragraph, in_cell: bool) -> Option<i32> {
    if in_cell || paragraph_is_image_only(p) || p.numbering.is_some() || p.code_block {
        None
    } else if p.outline_level.filter(|l| *l > 0).is_some() {
        Some(HEADING_LINE)
    } else {
        Some(BODY_LINE)
    }
}

fn spacing_xml(before: Hu, after: Hu, line: Option<i32>) -> String {
    if before <= 0 && after <= 0 && line.is_none() {
        return String::new();
    }
    let mut s = String::from("<w:spacing");
    if before > 0 {
        s.push_str(&format!(r#" w:before="{}""#, hu_to_twips(before)));
    }
    if after > 0 {
        s.push_str(&format!(r#" w:after="{}""#, hu_to_twips(after)));
    }
    if let Some(line) = line {
        s.push_str(&format!(r#" w:line="{line}" w:lineRule="auto""#));
    }
    s.push_str("/>");
    s
}

fn p_xml(
    p: &Paragraph,
    ctx: &mut WriteCtx,
    in_cell: bool,
    next: Option<&Block>,
    header_cell: bool,
) -> String {
    let mut s = String::from("<w:p>");
    let heading = p.outline_level.filter(|l| *l > 0);
    let num = p.numbering.as_ref();
    let space_before = paragraph_space_before(p);
    let space_after = paragraph_space_after(p, in_cell, next);
    let line = paragraph_line(p, in_cell);
    let has_spacing = space_before > 0 || space_after > 0 || line.is_some();
    if heading.is_some() || num.is_some() || p.quote || p.code_block || has_spacing {
        s.push_str("<w:pPr>");
        if let Some(level) = heading {
            let outline = level.saturating_sub(1);
            s.push_str(&format!(r#"<w:pStyle w:val="Heading{level}"/>"#));
            s.push_str(&heading_pbdr_xml(level));
            s.push_str(&format!(r#"<w:outlineLvl w:val="{outline}"/>"#));
        } else if num.is_some() && !p.quote && !p.code_block {
            s.push_str(r#"<w:pStyle w:val="ListParagraph"/>"#);
        }
        if p.quote {
            s.push_str(r#"<w:pStyle w:val="Quote"/>"#);
            s.push_str(&quote_pbdr_xml());
            s.push_str(&format!(r#"<w:ind w:left="{QUOTE_LEFT_TWIPS}"/>"#));
        }
        if p.code_block {
            s.push_str(&format!(
                r#"<w:pStyle w:val="CodeBlock"/>{}<w:shd w:val="clear" w:fill="{CODE_FILL}"/>{}"#,
                code_block_pbdr_xml(),
                code_block_ind_xml()
            ));
        }
        if let Some(n) = num {
            let num_id = match n.format {
                Some(NumberFormat::Decimal) => 2,
                Some(NumberFormat::Task) if n.start == Some(1) => TASK_CHECKED_NUM_ID,
                Some(NumberFormat::Task) => TASK_UNCHECKED_NUM_ID,
                _ => 1,
            };
            let ilvl = n.level;
            s.push_str(&format!(
                r#"<w:numPr><w:ilvl w:val="{ilvl}"/><w:numId w:val="{num_id}"/></w:numPr>"#
            ));
            if !p.quote {
                let left = LIST_LEFT_TWIPS.saturating_mul(i32::from(n.level).saturating_add(1));
                s.push_str(&format!(
                    r#"<w:ind w:left="{left}" w:hanging="{LIST_HANGING_TWIPS}"/>"#
                ));
            }
        }
        s.push_str(&spacing_xml(space_before, space_after, line));
        s.push_str("</w:pPr>");
    }
    let mut wrote = false;
    for run in &p.runs {
        match &run.content {
            RunContent::Inline(InlineObject::Hyperlink { display, .. }) => {
                if display.is_empty() {
                    continue;
                }
                let rid = ctx.next_link_rid();
                s.push_str(&format!(r#"<w:hyperlink r:id="rId{rid}">"#));
                s.push_str(r#"<w:r><w:rPr><w:rStyle w:val="Hyperlink"/>"#);
                s.push_str(&body_rfonts_xml());
                s.push_str(
                    r#"<w:u w:val="single"/><w:color w:val="0D9488"/></w:rPr><w:t xml:space="preserve">"#,
                );
                s.push_str(&xml_escape(display));
                s.push_str("</w:t></w:r></w:hyperlink>");
                wrote = true;
            }
            RunContent::Inline(InlineObject::Image(img)) => {
                let rid = ctx.next_image_rid();
                s.push_str(&drawing_run_xml(img, rid));
                wrote = true;
            }
            RunContent::Text(t) => {
                if t.is_empty() {
                    continue;
                }
                s.push_str("<w:r>");
                let sz = hu_to_half_points(run.style.size);
                let default_sz = hu_to_half_points(DEFAULT_FONT_SIZE_HU);
                // Heading styles own type size (python-docx / Pandoc). Emitting
                // markdown CSS `w:sz` 42/27 overrides Word 16pt/13pt.
                let emit_sz = heading.is_none() && sz != default_sz;
                // Letter CSS `th,td{font-size:.95rem}`. Direct w:sz so Word
                // paints 11.5pt even if TableGrid is ignored. Code chips keep
                // .92em; headings keep style size.
                let emit_cell_sz =
                    in_cell && heading.is_none() && !run.style.code && !p.code_block;
                // Letter CSS `th{color:#134e4a;font-weight:700}`. Skip code/mark
                // chips so the Consolas/amber rPr is not overwritten.
                let header_paint =
                    header_cell && !run.style.code && run.style.highlight.is_none();
                let emit_bold = run.style.bold || header_paint;
                // Direct rFonts so Word paints letter Segoe UI even if Normal
                // is ignored. Code chips / CodeBlock own Consolas.
                let emit_body_font = !run.style.code && !p.code_block;
                let emit_code_font = p.code_block && !run.style.code;
                // Letter CSS `h1{letter-spacing:-.02em}`. Direct rPr spacing so
                // Word tracks H1 even if Heading1 is ignored. H2 stays 0.
                let emit_h1_track = heading == Some(1);
                // Direct w:color so Word paints letter teal even if Normal is
                // ignored (python-docx / Pandoc). Heading styles own letter
                // #134e4a (not Word 2F5496). Code chips own CODE_COLOR. Mark
                // chips own MARK_COLOR (letter.html `mark{color:#134e4a}`).
                let run_color = if run.style.code || run.style.highlight.is_some() {
                    None
                } else if !color_is_black(run.style.color) {
                    Some(color_hex(run.style.color))
                } else if header_paint {
                    Some(TH_COLOR.to_string())
                } else if p.quote {
                    Some(QUOTE_COLOR.to_string())
                } else if heading.is_some() {
                    Some(HEADING_COLOR.to_string())
                } else {
                    Some(BODY_COLOR.to_string())
                };
                if emit_bold
                    || run.style.italic
                    || run.style.strike
                    || run.style.code
                    || run.style.highlight.is_some()
                    || run.style.underline != Underline::None
                    || run.style.superscript
                    || run.style.subscript
                    || emit_sz
                    || emit_cell_sz
                    || emit_body_font
                    || emit_code_font
                    || emit_h1_track
                    || run_color.is_some()
                {
                    s.push_str("<w:rPr>");
                    // rStyle first (ECMA-376). Code/Mark chips win over Strike/Underline/Super/Sub.
                    if run.style.code {
                        s.push_str(&code_rpr_xml());
                    } else if run.style.highlight.is_some() {
                        s.push_str(&mark_rpr_xml());
                    } else if run.style.strike {
                        s.push_str(strike_rstyle_xml());
                    } else if run.style.underline != Underline::None {
                        s.push_str(underline_rstyle_xml());
                    } else if run.style.superscript {
                        s.push_str(superscript_rstyle_xml());
                    } else if run.style.subscript {
                        s.push_str(subscript_rstyle_xml());
                    }
                    if emit_body_font {
                        s.push_str(&body_rfonts_xml());
                    } else if emit_code_font {
                        s.push_str(&code_block_rfonts_xml());
                    }
                    if emit_bold {
                        s.push_str("<w:b/>");
                    }
                    if run.style.italic {
                        s.push_str("<w:i/>");
                    }
                    if run.style.strike {
                        s.push_str("<w:strike/>");
                    }
                    if run.style.underline != Underline::None {
                        s.push_str(r#"<w:u w:val="single"/>"#);
                    }
                    if run.style.superscript {
                        s.push_str(r#"<w:vertAlign w:val="superscript"/>"#);
                    } else if run.style.subscript {
                        s.push_str(r#"<w:vertAlign w:val="subscript"/>"#);
                    }
                    if let Some(hex) = run_color.as_deref() {
                        s.push_str(&format!(r#"<w:color w:val="{hex}"/>"#));
                    }
                    if emit_h1_track {
                        s.push_str(&heading_spacing_xml(1));
                    }
                    if emit_sz {
                        s.push_str(&format!(r#"<w:sz w:val="{sz}"/><w:szCs w:val="{sz}"/>"#));
                    } else if emit_cell_sz {
                        s.push_str(&cell_sz_xml());
                    }
                    s.push_str("</w:rPr>");
                }
                s.push_str("<w:t xml:space=\"preserve\">");
                s.push_str(&xml_escape(t));
                s.push_str("</w:t></w:r>");
                wrote = true;
            }
            RunContent::Inline(_) => {}
        }
    }
    if !wrote {
        s.push_str("<w:r><w:t/></w:r>");
    }
    s.push_str("</w:p>");
    s
}

fn visible_cell_width(width: Hu) -> Hu {
    if width > 0 {
        width
    } else {
        10000
    }
}

fn grid_col_twips(table: &Table, content_width: Hu) -> Vec<i32> {
    let Some(row) = table.rows.first() else {
        return Vec::new();
    };
    let widths: Vec<i64> = row
        .cells
        .iter()
        .map(|c| i64::from(visible_cell_width(c.width)))
        .collect();
    if widths.is_empty() {
        return Vec::new();
    }
    let total: i64 = widths.iter().sum();
    let target_hu = i64::from(table.width.unwrap_or(content_width).max(1));
    let target_twips = hu_to_twips(table.width.unwrap_or(content_width).max(1)).max(1);
    let stretch = total > 0 && total < target_hu;
    let n = widths.len();
    let mut out = Vec::with_capacity(n);
    let mut used = 0i32;
    for (i, w) in widths.iter().enumerate() {
        let tw = if stretch {
            if i + 1 == n {
                (target_twips - used).max(1)
            } else {
                let t = (*w * i64::from(target_twips) / total).max(1);
                i32::try_from(t).unwrap_or(i32::MAX)
            }
        } else {
            hu_to_twips(i32::try_from(*w).unwrap_or(i32::MAX))
        };
        used = used.saturating_add(tw);
        out.push(tw);
    }
    out
}

fn color_hex(c: Color) -> String {
    format!("{:02X}{:02X}{:02X}", c.r, c.g, c.b)
}

fn color_is_black(c: Color) -> bool {
    c.r == 0 && c.g == 0 && c.b == 0
}

fn row_is_header(row: &TableRow, ri: usize, table: &Table) -> bool {
    row.header || ri < usize::from(table.header_row_count)
}

fn cell_shd_xml(cell: &TableCell, header: bool) -> String {
    let fill = cell
        .shading
        .map(color_hex)
        .or_else(|| header.then(|| TH_FILL.to_string()));
    match fill {
        Some(hex) => format!(r#"<w:shd w:val="clear" w:fill="{hex}"/>"#),
        None => String::new(),
    }
}

fn header_tc_borders_xml() -> String {
    let sz = hu_to_border_sz(TH_RULE_HU);
    format!(
        r#"<w:tcBorders><w:bottom w:val="single" w:sz="{sz}" w:space="0" w:color="{TH_RULE_COLOR}"/></w:tcBorders>"#
    )
}

fn ooxml_border_val(style: BorderStyle) -> &'static str {
    match style {
        BorderStyle::None => "nil",
        BorderStyle::Single => "single",
        BorderStyle::Double => "double",
        BorderStyle::Dotted => "dotted",
        BorderStyle::Dashed => "dashed",
        BorderStyle::Thick => "thick",
    }
}

fn hu_to_border_sz(hu: Hu) -> i32 {
    // OOXML border sz is eighths of a point; Word hides sz below ~4 (0.5pt).
    let eighths = (i64::from(hu.max(0)) * 8) / i64::from(HU_PER_POINT);
    i32::try_from(eighths).unwrap_or(i32::MAX).max(4)
}

/// Letter CSS `th,td{border:1px solid #cbd5e1}`. IR/md default black becomes
/// Word TableGrid `000000`; remap so the hop grid matches letter.html.
fn tbl_border_sz_color(b: &Border) -> (i32, String) {
    if color_is_black(b.color) {
        (
            hu_to_border_sz(TBL_BORDER_HU),
            TBL_BORDER_COLOR.to_string(),
        )
    } else {
        (hu_to_border_sz(b.width), color_hex(b.color))
    }
}

fn tbl_borders_xml(borders: &TableBorders) -> String {
    let mut s = String::from("<w:tblBorders>");
    for (name, b) in [
        ("top", &borders.top),
        ("left", &borders.left),
        ("bottom", &borders.bottom),
        ("right", &borders.right),
        ("insideH", &borders.inside_h),
        ("insideV", &borders.inside_v),
    ] {
        let (sz, color) = tbl_border_sz_color(b);
        s.push_str(&format!(
            r#"<w:{name} w:val="{}" w:sz="{sz}" w:space="0" w:color="{color}"/>"#,
            ooxml_border_val(b.style),
        ));
    }
    s.push_str("</w:tblBorders>");
    s
}

fn cell_pads(cell: &TableCell) -> (Hu, Hu, Hu, Hu) {
    let p = cell.padding;
    (
        if p.left > 0 { p.left } else { CELL_PAD_X_HU },
        if p.right > 0 { p.right } else { CELL_PAD_X_HU },
        if p.top > 0 { p.top } else { CELL_PAD_Y_HU },
        if p.bottom > 0 { p.bottom } else { CELL_PAD_Y_HU },
    )
}

fn mar_sides_xml(left: Hu, right: Hu, top: Hu, bottom: Hu) -> String {
    let l = hu_to_twips(left);
    let r = hu_to_twips(right);
    let t = hu_to_twips(top);
    let b = hu_to_twips(bottom);
    format!(
        r#"<w:top w:w="{t}" w:type="dxa"/><w:left w:w="{l}" w:type="dxa"/><w:bottom w:w="{b}" w:type="dxa"/><w:right w:w="{r}" w:type="dxa"/>"#
    )
}

fn tbl_cell_mar_xml() -> String {
    format!(
        "<w:tblCellMar>{}</w:tblCellMar>",
        mar_sides_xml(CELL_PAD_X_HU, CELL_PAD_X_HU, CELL_PAD_Y_HU, CELL_PAD_Y_HU)
    )
}

fn tc_mar_xml(cell: &TableCell) -> String {
    let (left, right, top, bottom) = cell_pads(cell);
    format!("<w:tcMar>{}</w:tcMar>", mar_sides_xml(left, right, top, bottom))
}

/// Letter CSS `table{border-collapse:collapse}`. Direct so Word 2003 compat
/// cannot paint 2px hop gutters even if TableGrid is ignored.
fn tbl_cell_spacing_xml() -> &'static str {
    r#"<w:tblCellSpacing w:w="0" w:type="dxa"/>"#
}

fn tbl_pr_xml(table: &Table, col_twips: &[i32]) -> String {
    let grid_twips: i32 = col_twips.iter().copied().sum();
    let mut s = String::from("<w:tblPr>");
    s.push_str(r#"<w:tblStyle w:val="TableGrid"/>"#);
    if grid_twips > 0 {
        s.push_str(&format!(
            r#"<w:tblW w:w="{grid_twips}" w:type="dxa"/>"#
        ));
    }
    s.push_str(tbl_cell_spacing_xml());
    s.push_str(&tbl_borders_xml(&table.borders));
    s.push_str(&tbl_cell_mar_xml());
    s.push_str(
        r#"<w:tblLook w:firstRow="1" w:lastRow="0" w:firstColumn="0" w:lastColumn="0" w:noHBand="0" w:noVBand="1" w:val="04A0"/>"#,
    );
    s.push_str("</w:tblPr>");
    s
}

fn tbl_xml(table: &Table, ctx: &mut WriteCtx, content_width: Hu) -> String {
    let col_twips = grid_col_twips(table, content_width);
    let mut s = String::from("<w:tbl>");
    s.push_str(&tbl_pr_xml(table, &col_twips));
    s.push_str("<w:tblGrid>");
    for w in &col_twips {
        s.push_str(&format!(r#"<w:gridCol w:w="{w}"/>"#));
    }
    s.push_str("</w:tblGrid>");
    for (ri, row) in table.rows.iter().enumerate() {
        let header = row_is_header(row, ri, table);
        s.push_str("<w:tr>");
        if header {
            s.push_str(r#"<w:trPr><w:tblHeader/></w:trPr>"#);
        }
        for (ci, cell) in row.cells.iter().enumerate() {
            let shd = cell_shd_xml(cell, header);
            let borders = if header {
                header_tc_borders_xml()
            } else {
                String::new()
            };
            let cell_w = col_twips
                .get(ci)
                .copied()
                .unwrap_or_else(|| hu_to_twips(visible_cell_width(cell.width)));
            let mar = tc_mar_xml(cell);
            let valign = cell_valign_xml();
            s.push_str(&format!(
                r#"<w:tc><w:tcPr><w:tcW w:w="{cell_w}" w:type="dxa"/>{borders}{shd}{mar}{valign}</w:tcPr>"#
            ));
            let mut wrote_p = false;
            for (bi, b) in cell.blocks.iter().enumerate() {
                match b {
                    Block::Paragraph(p) => {
                        s.push_str(&p_xml(p, ctx, true, cell.blocks.get(bi + 1), header));
                        wrote_p = true;
                    }
                    Block::Float(Float::Image(img)) => {
                        s.push_str(&float_image_p_xml(img, ctx));
                        wrote_p = true;
                    }
                    _ => {}
                }
            }
            if !wrote_p {
                s.push_str("<w:p><w:r><w:t/></w:r></w:p>");
            }
            s.push_str("</w:tc>");
        }
        s.push_str("</w:tr>");
    }
    s.push_str("</w:tbl>");
    s
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

struct PendingPic {
    embed: Option<String>,
    cx: Option<i64>,
    cy: Option<i64>,
    alt: Option<String>,
    wrap: WrapMode,
}

impl PendingPic {
    fn new() -> Self {
        Self {
            embed: None,
            cx: None,
            cy: None,
            alt: None,
            wrap: WrapMode::Inline,
        }
    }
}

fn parse_document_xml(
    xml: &str,
    rels: &HashMap<String, Rel>,
    media: &HashMap<String, MediaPart>,
    styles: &HashMap<String, StyleNum>,
) -> Result<Document, Error> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut section = Section::default();
    let mut body = Vec::new();
    let mut in_t = false;
    let mut in_run = false;
    let mut in_rpr = false;
    let mut in_ppr = false;
    let mut in_tbl = false;
    let mut run_bold = false;
    let mut run_italic = false;
    let mut run_strike = false;
    let mut run_code = false;
    let mut run_mark = false;
    let mut run_under = false;
    let mut run_super = false;
    let mut run_sub = false;
    let mut run_size: Option<i32> = None;
    let mut run_text = String::new();
    let mut pending_pic: Option<PendingPic> = None;
    let mut run_had_drawing = false;
    let mut para_runs: Vec<Run> = Vec::new();
    let mut para_outline: Option<u8> = None;
    let mut para_quote = false;
    let mut para_code_block = false;
    let mut para_hr = false;
    let mut para_num_id: Option<u32> = None;
    let mut para_ilvl: u8 = 0;
    let mut para_ilvl_seen = false;
    let mut para_style: Option<String> = None;
    let mut para_in_checkbox = false;
    let mut para_checkbox: Option<bool> = None;
    let mut decimal_seq: u32 = 1;
    let mut table_rows: Vec<TableRow> = Vec::new();
    let mut cur_row: Vec<TableCell> = Vec::new();
    let mut cell_blocks: Vec<Block> = Vec::new();
    let mut cell_width = 10000i32;
    let mut hyperlink_target: Option<String> = None;
    let mut row_header = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = local_name(&e);
                match name.as_str() {
                    "t" => in_t = true,
                    "pPr" => in_ppr = true,
                    "pStyle" if in_ppr => {
                        if let Some(v) = attr(&e, "val") {
                            para_style = Some(v.clone());
                            if let Some(level) = heading_level_from_style(&v) {
                                para_outline = Some(level);
                            } else if v.eq_ignore_ascii_case("Quote") {
                                para_quote = true;
                            } else if v.eq_ignore_ascii_case("CodeBlock") {
                                para_code_block = true;
                            } else if v.eq_ignore_ascii_case("HorizontalLine") {
                                para_hr = true;
                            }
                        }
                    }
                    "outlineLvl" if in_ppr => {
                        if let Some(v) = attr(&e, "val").and_then(|v| v.parse::<u8>().ok()) {
                            para_outline = Some(v.saturating_add(1));
                        }
                    }
                    "ilvl" if in_ppr => {
                        if let Some(v) = attr(&e, "val").and_then(|v| v.parse::<u8>().ok()) {
                            para_ilvl = v;
                            para_ilvl_seen = true;
                        }
                    }
                    "numId" if in_ppr => {
                        para_num_id = attr(&e, "val").and_then(|v| v.parse().ok());
                    }
                    "rPr" if in_run => in_rpr = true,
                    "b" if in_rpr => run_bold = ooxml_on(&e),
                    "i" if in_rpr => run_italic = ooxml_on(&e),
                    "bottom" if in_ppr => para_hr = true,
                    "strike" if in_rpr => run_strike = ooxml_on(&e),
                    "rStyle" if in_rpr => {
                        match attr(&e, "val").as_deref() {
                            Some("Code") => run_code = true,
                            Some("Mark") | Some("Highlight") => run_mark = true,
                            Some("Strike") | Some("Strikethrough") => run_strike = true,
                            Some("Underline") | Some("Hyperlink") => run_under = true,
                            Some("Superscript") => run_super = true,
                            Some("Subscript") => run_sub = true,
                            _ => {}
                        }
                    }
                    "shd" if in_rpr => {
                        let fill = attr(&e, "fill").unwrap_or_default();
                        if fill.eq_ignore_ascii_case(CODE_FILL) {
                            run_code = true;
                        } else if fill.eq_ignore_ascii_case(MARK_FILL) {
                            run_mark = true;
                        }
                    }
                    "highlight" if in_rpr => {
                        let v = attr(&e, "val").unwrap_or_default();
                        run_mark = !v.is_empty() && !v.eq_ignore_ascii_case("none");
                    }
                    "u" if in_rpr => {
                        let v = attr(&e, "val").unwrap_or_else(|| "single".into());
                        run_under = !v.eq_ignore_ascii_case("none");
                    }
                    "vertAlign" if in_rpr => {
                        let v = attr(&e, "val");
                        run_super = v.as_deref() == Some("superscript");
                        run_sub = v.as_deref() == Some("subscript");
                    }
                    "sz" if in_rpr => {
                        if let Some(v) = attr(&e, "val").and_then(|v| v.parse::<i32>().ok()) {
                            run_size = Some(half_points_to_hu(v));
                        }
                    }
                    "hyperlink" => {
                        let id = attr(&e, "id").unwrap_or_default();
                        hyperlink_target = rels.get(&id).and_then(|r| {
                            r.ty.contains("hyperlink").then(|| r.target.clone())
                        });
                    }
                    "drawing" => {
                        flush_run(
                            &mut para_runs,
                            &mut run_text,
                            RunMarks {
                                bold: run_bold,
                                italic: run_italic,
                                strike: run_strike,
                                code: run_code,
                                highlight: run_mark,
                                underline: run_under,
                                superscript: run_super,
                                subscript: run_sub,
                                size: run_size,
                            },
                            hyperlink_target.as_deref(),
                        );
                        pending_pic = Some(PendingPic::new());
                    }
                    "pict" => {
                        if !run_had_drawing {
                            flush_run(
                                &mut para_runs,
                                &mut run_text,
                                RunMarks {
                                    bold: run_bold,
                                    italic: run_italic,
                                    strike: run_strike,
                                    code: run_code,
                                    highlight: run_mark,
                                    underline: run_under,
                                    superscript: run_super,
                                    subscript: run_sub,
                                    size: run_size,
                                },
                                hyperlink_target.as_deref(),
                            );
                            pending_pic = Some(PendingPic::new());
                        }
                    }
                    "inline" => {
                        if let Some(p) = pending_pic.as_mut() {
                            p.wrap = WrapMode::Inline;
                        }
                    }
                    "anchor" => {
                        if let Some(p) = pending_pic.as_mut() {
                            p.wrap = if attr(&e, "behindDoc").as_deref() == Some("1") {
                                WrapMode::Behind
                            } else {
                                WrapMode::InFront
                            };
                        }
                    }
                    "wrapSquare" => {
                        if let Some(p) = pending_pic.as_mut() {
                            p.wrap = WrapMode::Square;
                        }
                    }
                    "wrapTight" => {
                        if let Some(p) = pending_pic.as_mut() {
                            p.wrap = WrapMode::Tight;
                        }
                    }
                    "wrapThrough" => {
                        if let Some(p) = pending_pic.as_mut() {
                            p.wrap = WrapMode::Through;
                        }
                    }
                    "wrapTopAndBottom" => {
                        if let Some(p) = pending_pic.as_mut() {
                            p.wrap = WrapMode::TopAndBottom;
                        }
                    }
                    "wrapNone" => {
                        if let Some(p) = pending_pic.as_mut()
                            && p.wrap != WrapMode::Behind
                        {
                            p.wrap = WrapMode::InFront;
                        }
                    }
                    "extent" | "ext" => {
                        if let Some(p) = pending_pic.as_mut() {
                            if let Some(cx) = attr(&e, "cx").and_then(|v| v.parse().ok()) {
                                p.cx = Some(cx);
                            }
                            if let Some(cy) = attr(&e, "cy").and_then(|v| v.parse().ok()) {
                                p.cy = Some(cy);
                            }
                        }
                    }
                    "docPr" | "cNvPr" => {
                        if let Some(p) = pending_pic.as_mut() {
                            let descr = attr(&e, "descr").or_else(|| attr(&e, "title"));
                            if let Some(d) = descr.filter(|s| !s.is_empty()) {
                                p.alt = Some(d);
                            }
                        }
                    }
                    "blip" => {
                        if let Some(p) = pending_pic.as_mut() {
                            p.embed = attr(&e, "embed");
                        }
                    }
                    "imagedata" => {
                        if pending_pic.is_none() && !run_had_drawing {
                            pending_pic = Some(PendingPic::new());
                        }
                        if let Some(p) = pending_pic.as_mut() {
                            p.embed = attr(&e, "id")
                                .or_else(|| attr(&e, "relid"))
                                .or_else(|| attr(&e, "href"));
                        }
                    }
                    "sym" => {
                        if let (Some(font), Some(ch)) = (attr(&e, "font"), attr(&e, "char"))
                            && let Some(glyph) = sym_checkbox_glyph(&font, &ch)
                        {
                            run_text.push_str(glyph);
                        }
                    }
                    "checkbox" | "checkBox" => {
                        para_in_checkbox = true;
                        if para_checkbox.is_none() {
                            para_checkbox = Some(false);
                        }
                    }
                    "checked" if para_in_checkbox => {
                        para_checkbox = Some(ooxml_on(&e));
                    }
                    "r" => {
                        in_run = true;
                        run_had_drawing = false;
                        run_bold = false;
                        run_italic = false;
                        run_strike = false;
                        run_code = false;
                        run_mark = false;
                        run_under = false;
                        run_super = false;
                        run_sub = false;
                        run_size = None;
                        run_text.clear();
                    }
                    "p" => {
                        para_runs.clear();
                        para_outline = None;
                        para_quote = false;
                        para_code_block = false;
                        para_hr = false;
                        para_num_id = None;
                        para_ilvl = 0;
                        para_ilvl_seen = false;
                        para_style = None;
                        para_in_checkbox = false;
                        para_checkbox = None;
                        run_bold = false;
                        run_italic = false;
                        run_strike = false;
                        run_code = false;
                        run_mark = false;
                        run_under = false;
                        run_super = false;
                        run_sub = false;
                        run_size = None;
                        run_text.clear();
                    }
                    "tbl" => {
                        if let Some(mut p) = take_para(
                            &mut para_runs,
                            &mut para_outline,
                            &mut para_quote,
                            &mut para_code_block,
                            &mut para_num_id,
                            &mut para_ilvl,
                            &mut decimal_seq,
                        ) {
                            apply_task_marker(&mut p, para_checkbox.take());
                            body.push(Block::Paragraph(p));
                        }
                        in_tbl = true;
                    }
                    "tr" if in_tbl => {
                        cur_row.clear();
                        row_header = false;
                    }
                    "tblHeader" => row_header = true,
                    "tc" if in_tbl => {
                        cell_blocks.clear();
                        cell_width = 10000;
                    }
                    "tcW" => {
                        if let Some(w) = attr(&e, "w")
                            && let Ok(tw) = w.parse::<i32>()
                        {
                            cell_width = twips_to_hu(tw);
                        }
                    }
                    "pgSz" => {
                        if let Some(w) = attr(&e, "w").and_then(|v| v.parse().ok()) {
                            section.page.width = twips_to_hu(w);
                        }
                        if let Some(h) = attr(&e, "h").and_then(|v| v.parse().ok()) {
                            section.page.height = twips_to_hu(h);
                        }
                    }
                    "pgMar" => {
                        if let Some(v) = attr(&e, "left").and_then(|v| v.parse().ok()) {
                            section.page.margin_left = twips_to_hu(v);
                        }
                        if let Some(v) = attr(&e, "right").and_then(|v| v.parse().ok()) {
                            section.page.margin_right = twips_to_hu(v);
                        }
                        if let Some(v) = attr(&e, "top").and_then(|v| v.parse().ok()) {
                            section.page.margin_top = twips_to_hu(v);
                        }
                        if let Some(v) = attr(&e, "bottom").and_then(|v| v.parse().ok()) {
                            section.page.margin_bottom = twips_to_hu(v);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                match name.as_str() {
                    "t" => in_t = false,
                    "pPr" => {
                        in_ppr = false;
                        if para_num_id.is_none()
                            && let Some(sid) = para_style.as_deref()
                        {
                            let (nid, lvl) = style_numbering(styles, sid);
                            if let Some(id) = nid {
                                para_num_id = Some(id);
                                if !para_ilvl_seen {
                                    para_ilvl = lvl;
                                }
                            }
                        }
                    }
                    "rPr" => in_rpr = false,
                    "drawing" => {
                        flush_pic(&mut para_runs, &mut pending_pic, media);
                        run_had_drawing = true;
                    }
                    "pict" => {
                        if !run_had_drawing {
                            flush_pic(&mut para_runs, &mut pending_pic, media);
                        } else {
                            pending_pic = None;
                        }
                    }
                    "r" => {
                        flush_pic(&mut para_runs, &mut pending_pic, media);
                        flush_run(
                            &mut para_runs,
                            &mut run_text,
                            RunMarks {
                                bold: run_bold,
                                italic: run_italic,
                                strike: run_strike,
                                code: run_code,
                                highlight: run_mark,
                                underline: run_under,
                                superscript: run_super,
                                subscript: run_sub,
                                size: run_size,
                            },
                            hyperlink_target.as_deref(),
                        );
                        in_run = false;
                        run_bold = false;
                        run_italic = false;
                        run_strike = false;
                        run_code = false;
                        run_mark = false;
                        run_under = false;
                        run_super = false;
                        run_sub = false;
                        run_size = None;
                    }
                    "hyperlink" => {
                        hyperlink_target = None;
                    }
                    "checkbox" | "checkBox" => {
                        para_in_checkbox = false;
                    }
                    "p" => {
                        flush_run(
                            &mut para_runs,
                            &mut run_text,
                            RunMarks {
                                bold: run_bold,
                                italic: run_italic,
                                strike: run_strike,
                                code: run_code,
                                highlight: run_mark,
                                underline: run_under,
                                superscript: run_super,
                                subscript: run_sub,
                                size: run_size,
                            },
                            hyperlink_target.as_deref(),
                        );
                        if para_hr && para_runs.is_empty() {
                            para_hr = false;
                            if !in_tbl {
                                body.push(Block::Break(BreakKind::Thematic));
                            }
                        } else if let Some(mut p) = take_para(
                            &mut para_runs,
                            &mut para_outline,
                            &mut para_quote,
                            &mut para_code_block,
                            &mut para_num_id,
                            &mut para_ilvl,
                            &mut decimal_seq,
                        ) {
                            apply_task_marker(&mut p, para_checkbox.take());
                            if in_tbl {
                                cell_blocks.push(Block::Paragraph(p));
                            } else {
                                body.push(Block::Paragraph(p));
                            }
                        }
                    }
                    "tc" if in_tbl => {
                        if let Some(mut p) = take_para(
                            &mut para_runs,
                            &mut para_outline,
                            &mut para_quote,
                            &mut para_code_block,
                            &mut para_num_id,
                            &mut para_ilvl,
                            &mut decimal_seq,
                        ) {
                            apply_task_marker(&mut p, para_checkbox.take());
                            cell_blocks.push(Block::Paragraph(p));
                        }
                        cur_row.push(TableCell {
                            width: cell_width,
                            blocks: std::mem::take(&mut cell_blocks),
                            ..TableCell::default()
                        });
                    }
                    "tr" if in_tbl => table_rows.push(TableRow {
                        cells: std::mem::take(&mut cur_row),
                        height: None,
                        header: row_header,
                        cant_split: None,
                    }),
                    "tbl" => {
                        in_tbl = false;
                        let rows = std::mem::take(&mut table_rows);
                        let header_row_count =
                            rows.iter().take_while(|r| r.header).count() as u8;
                        body.push(Block::Table(Table {
                            rows,
                            alignment: Alignment::Start,
                            header_row_count,
                            ..Table::from_cells(Vec::new())
                        }));
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(t)) => {
                if in_t {
                    let decoded = t.unescape().map_err(|e| Error::Xml(e.to_string()))?;
                    run_text.push_str(&decoded);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(Error::Xml(e.to_string())),
            _ => {}
        }
        buf.clear();
    }
    flush_pic(&mut para_runs, &mut pending_pic, media);
    flush_run(
        &mut para_runs,
        &mut run_text,
        RunMarks {
            bold: run_bold,
            italic: run_italic,
            strike: run_strike,
            code: run_code,
            highlight: run_mark,
            underline: run_under,
            superscript: run_super,
            subscript: run_sub,
            size: run_size,
        },
        hyperlink_target.as_deref(),
    );
    if let Some(mut p) = take_para(
        &mut para_runs,
        &mut para_outline,
        &mut para_quote,
        &mut para_code_block,
        &mut para_num_id,
        &mut para_ilvl,
        &mut decimal_seq,
    ) {
        apply_task_marker(&mut p, para_checkbox.take());
        body.push(Block::Paragraph(p));
    }
    section.body = body;
    Ok(Document {
        sections: vec![section],
        ..Document::default()
    })
}

fn heading_level_from_style(name: &str) -> Option<u8> {
    let compact: String = name.chars().filter(|c| !c.is_whitespace()).collect();
    let rest = compact
        .strip_prefix("Heading")
        .or_else(|| compact.strip_prefix("heading"))?;
    rest.parse::<u8>().ok().filter(|v| (1..=9).contains(v))
}

#[derive(Clone, Copy, Default)]
struct RunMarks {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
    highlight: bool,
    underline: bool,
    superscript: bool,
    subscript: bool,
    size: Option<i32>,
}

fn flush_pic(
    runs: &mut Vec<Run>,
    pic: &mut Option<PendingPic>,
    media: &HashMap<String, MediaPart>,
) {
    let Some(pic) = pic.take() else {
        return;
    };
    let Some(id) = pic.embed else {
        return;
    };
    let Some(part) = media.get(&id) else {
        return;
    };
    let width = pic.cx.map(emu_to_hu).filter(|w| *w > 0).unwrap_or(HU_PER_INCH);
    let height = pic.cy.map(emu_to_hu).filter(|h| *h > 0).unwrap_or(HU_PER_INCH);
    runs.push(Run {
        style: CharStyle::default(),
        content: RunContent::Inline(InlineObject::Image(ImageData {
            bytes: part.bytes.clone(),
            mime: part.mime.clone(),
            width,
            height,
            alt_text: pic.alt.filter(|s| !s.is_empty()),
            wrap: pic.wrap,
        })),
    });
}

fn flush_run(runs: &mut Vec<Run>, text: &mut String, marks: RunMarks, href: Option<&str>) {
    if text.is_empty() {
        return;
    }
    let mut run = if let Some(url) = href.filter(|u| !u.is_empty()) {
        Run::hyperlink(std::mem::take(text), url)
    } else {
        Run::text(std::mem::take(text))
    };
    run.style.bold = marks.bold;
    run.style.italic = marks.italic;
    run.style.strike = marks.strike;
    run.style.code = marks.code;
    if marks.highlight {
        run.style.highlight = Some(docagent_model::Color::MARK);
    }
    if marks.underline {
        run.style.underline = Underline::Single;
    }
    run.style.superscript = marks.superscript;
    run.style.subscript = marks.subscript;
    if let Some(sz) = marks.size {
        run.style.size = sz;
    }
    runs.push(run);
}

fn task_prefix_kind(text: &str) -> Option<(bool, usize)> {
    const CHECKED: &[&str] = &[
        "[x] ",
        "[X] ",
        "\u{2612} ",
        "\u{2611} ",
        "\u{2612}",
        "\u{2611}",
    ];
    const UNCHECKED: &[&str] = &["[ ] ", "\u{2610} ", "\u{2610}"];
    for prefix in CHECKED {
        if text.starts_with(*prefix) {
            return Some((true, prefix.len()));
        }
    }
    for prefix in UNCHECKED {
        if text.starts_with(*prefix) {
            return Some((false, prefix.len()));
        }
    }
    None
}

fn drop_leading_blank_text_runs(p: &mut Paragraph) {
    while let Some(Run {
        content: RunContent::Text(t),
        ..
    }) = p.runs.first()
    {
        if t.is_empty() || t.chars().all(char::is_whitespace) {
            p.runs.remove(0);
        } else {
            break;
        }
    }
}

fn strip_leading_task_glyph(p: &mut Paragraph) -> Option<bool> {
    let run = p.runs.first_mut()?;
    let RunContent::Text(text) = &mut run.content else {
        return None;
    };
    let (checked, n) = task_prefix_kind(text)?;
    *text = text[n..].to_string();
    drop_leading_blank_text_runs(p);
    Some(checked)
}

fn apply_task_marker(p: &mut Paragraph, sdt_checked: Option<bool>) {
    let from_text = strip_leading_task_glyph(p);
    let Some(checked) = sdt_checked.or(from_text) else {
        return;
    };
    let level = p.numbering.as_ref().map(|n| n.level).unwrap_or(0);
    p.numbering = Some(NumberingRef {
        definition_id: 2,
        level,
        start: checked.then_some(1),
        format: Some(NumberFormat::Task),
    });
}

fn parse_hex_u32(s: &str) -> Option<u32> {
    let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    u32::from_str_radix(s, 16).ok()
}

fn sym_checkbox_glyph(font: &str, char_hex: &str) -> Option<&'static str> {
    let font = font.to_ascii_lowercase();
    if !font.contains("wingdings") {
        return None;
    }
    let code = parse_hex_u32(char_hex)?;
    match code & 0xFF {
        0xA8 | 0xA3 | 0xA4 | 0x6F => Some("\u{2610}"),
        0xFE | 0x52 | 0x53 | 0x78 => Some("\u{2612}"),
        _ => None,
    }
}

fn take_para(
    runs: &mut Vec<Run>,
    outline: &mut Option<u8>,
    quote: &mut bool,
    code_block: &mut bool,
    num_id: &mut Option<u32>,
    ilvl: &mut u8,
    decimal_seq: &mut u32,
) -> Option<Paragraph> {
    if runs.is_empty() {
        *outline = None;
        *quote = false;
        *code_block = false;
        *num_id = None;
        *ilvl = 0;
        return None;
    }
    let mut p = Paragraph::from_text("");
    p.runs = std::mem::take(runs);
    p.outline_level = outline.take();
    p.quote = std::mem::take(quote);
    p.code_block = std::mem::take(code_block);
    if let Some(level) = p.outline_level.filter(|l| *l > 0) {
        let sz = heading_style_size_hu(level);
        for run in &mut p.runs {
            if run.style.size == DEFAULT_FONT_SIZE_HU {
                run.style.size = sz;
            }
        }
    }
    if let Some(id) = num_id.take() {
        let format = match id {
            2 => NumberFormat::Decimal,
            TASK_UNCHECKED_NUM_ID | TASK_CHECKED_NUM_ID => NumberFormat::Task,
            _ => NumberFormat::Bullet,
        };
        let start = if format == NumberFormat::Decimal && *ilvl == 0 {
            let n = *decimal_seq;
            *decimal_seq = decimal_seq.saturating_add(1);
            Some(n)
        } else if format == NumberFormat::Task && id == TASK_CHECKED_NUM_ID {
            Some(1)
        } else {
            None
        };
        if matches!(format, NumberFormat::Bullet | NumberFormat::Task) && *ilvl == 0 {
            *decimal_seq = 1;
        }
        p.numbering = Some(NumberingRef {
            definition_id: id,
            level: *ilvl,
            start,
            format: Some(format),
        });
    } else {
        *decimal_seq = 1;
    }
    *ilvl = 0;
    Some(p)
}

fn ooxml_on(e: &BytesStart<'_>) -> bool {
    !matches!(
        attr(e, "val").as_deref(),
        Some("0") | Some("false") | Some("off")
    )
}

fn local_name(e: &BytesStart<'_>) -> String {
    String::from_utf8_lossy(e.local_name().as_ref()).into_owned()
}

fn attr(e: &BytesStart<'_>, key: &str) -> Option<String> {
    e.try_get_attribute(key)
        .ok()
        .flatten()
        .map(|a| String::from_utf8_lossy(&a.value).into_owned())
        .or_else(|| {
            e.try_get_attribute(format!("w:{key}"))
                .ok()
                .flatten()
                .map(|a| String::from_utf8_lossy(&a.value).into_owned())
        })
        .or_else(|| {
            e.try_get_attribute(format!("r:{key}"))
                .ok()
                .flatten()
                .map(|a| String::from_utf8_lossy(&a.value).into_owned())
        })
        .or_else(|| {
            e.attributes()
                .filter_map(|a| a.ok())
                .find(|a| a.key.local_name().as_ref() == key.as_bytes())
                .map(|a| String::from_utf8_lossy(&a.value).into_owned())
        })
}

/// Structured loss list for HWP ↔ DOCX. Both sides of a missing concept are
/// explicit `Option` fields on the IR; this function reports what the other
/// format cannot represent losslessly.
pub fn loss_diagnostics(from_hwp: &Document, from_docx: &Document) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    if from_hwp.plain_text() != from_docx.plain_text() {
        out.push(Diagnostic::roundtrip_loss(format!(
            "plain text diverged (hwp {} chars, docx {} chars)",
            from_hwp.plain_text().chars().count(),
            from_docx.plain_text().chars().count()
        )));
    }
    let hwp_hints: usize = from_hwp
        .sections
        .iter()
        .flat_map(|s| s.body.iter())
        .map(|b| match b {
            Block::Paragraph(p) => p.layout_hints.len(),
            _ => 0,
        })
        .sum();
    if hwp_hints > 0 {
        out.push(Diagnostic {
            severity: Severity::Info,
            code: DiagnosticCode::RoundtripLoss,
            message: format!(
                "LineSeg layout hints ({hwp_hints}) have no OOXML equivalent and are dropped on DOCX export"
            ),
            location: Some("paragraph.layout_hints".into()),
        });
    }
    for (i, section) in from_hwp.sections.iter().enumerate() {
        if section.background.is_some() {
            out.push(Diagnostic {
                severity: Severity::Warning,
                code: DiagnosticCode::RoundtripLoss,
                message: format!("section {i} background/바탕쪽 is not represented in DOCX export"),
                location: Some(format!("sections[{i}].background")),
            });
        }
        if !section.endnotes.is_empty() {
            out.push(Diagnostic {
                severity: Severity::Info,
                code: DiagnosticCode::RoundtripLoss,
                message: format!(
                    "section {i} endnotes ({}) require w:endnote part not emitted in v1 writer",
                    section.endnotes.len()
                ),
                location: Some(format!("sections[{i}].endnotes")),
            });
        }
    }
    if from_hwp.table_count() != from_docx.table_count() {
        out.push(Diagnostic::roundtrip_loss(format!(
            "table count hwp={} docx={}",
            from_hwp.table_count(),
            from_docx.table_count()
        )));
    }
    out
}

pub fn hwp_to_docx_with_loss(hwp_ir: &Document) -> Result<(Vec<u8>, Vec<Diagnostic>), Error> {
    let bytes = write(hwp_ir)?;
    let back = read(&bytes)?;
    Ok((bytes, loss_diagnostics(hwp_ir, &back)))
}

pub fn roundtrip(doc: &Document) -> Result<Document, Error> {
    read(&write(doc)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read, Write};

    use docagent_model::{
        CharStyle, Hu, ImageData, InlineObject, LayoutHint, NumberFormat, NumberingRef, Paragraph,
        Run, RunContent, WrapMode, HU_PER_INCH,
    };
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    fn mark_png() -> Vec<u8> {
        include_bytes!("../../../docs/assets/mark.png").to_vec()
    }

    /// Valid 1×1 grayscale JPEG (SOI … EOI). Not `mark.png`.
    fn tiny_jpeg() -> Vec<u8> {
        vec![
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00,
            0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00,
            0x01, 0x00, 0x01, 0x01, 0x01, 0x11, 0x00, 0xFF, 0xC4, 0x00, 0x14, 0x00, 0x01, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0xFF, 0xC4, 0x00, 0x14, 0x10, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xDA, 0x00, 0x08, 0x01,
            0x01, 0x00, 0x00, 0x3F, 0x00, 0x3F, 0xFF, 0xD9,
        ]
    }

    fn image_run(bytes: Vec<u8>, width: Hu, height: Hu, alt: &str) -> Run {
        Run {
            style: CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(ImageData {
                bytes,
                mime: "image/png".into(),
                width,
                height,
                alt_text: Some(alt.into()),
                wrap: WrapMode::Inline,
            })),
        }
    }

    fn jpeg_run(bytes: Vec<u8>, width: Hu, height: Hu, alt: &str) -> Run {
        Run {
            style: CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(ImageData {
                bytes,
                mime: "image/jpeg".into(),
                width,
                height,
                alt_text: Some(alt.into()),
                wrap: WrapMode::Inline,
            })),
        }
    }

    fn pack_docx(parts: &[(&str, &[u8])]) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut cursor);
            let opt = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            for (name, data) in parts {
                zip.start_file(*name, opt).unwrap();
                zip.write_all(data).unwrap();
            }
            zip.finish().unwrap();
        }
        cursor.into_inner()
    }

    const MINIMAL_CONTENT_TYPES: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
    const MINIMAL_RELS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;

    #[test]
    fn roundtrip_keeps_bold_and_italic() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("plain ");
        let mut bold = Run::text("GPU-free");
        bold.style.bold = true;
        p.runs.push(bold);
        let mut italic = Run::text(" byte-for-byte");
        italic.style.italic = true;
        p.runs.push(italic);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = document_xml(&doc);
        assert!(xml.contains("<w:b/>"), "docx must emit w:b");
        assert!(xml.contains("<w:i/>"), "docx must emit w:i");
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.bold && matches!(&r.content, RunContent::Text(t) if t.contains("GPU-free"))
        }));
        assert!(p.runs.iter().any(|r| {
            r.style.italic && matches!(&r.content, RunContent::Text(t) if t.contains("byte-for-byte"))
        }));
    }

    #[test]
    fn roundtrip_keeps_quote() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        p.quote = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = document_xml(&doc);
        let quote_p = para_containing(&xml, "Replay is evidence");
        let bar_sz = hu_to_border_sz(QUOTE_BAR_HU);
        assert!(
            quote_p.contains(r#"w:val="Quote""#)
                && quote_p.contains("<w:pBdr>")
                && quote_p.contains("<w:left ")
                && quote_p.contains(&format!(r#"w:sz="{bar_sz}""#))
                && quote_p.contains(&format!(r#"w:color="{QUOTE_COLOR}""#)),
            "blockquote must emit Word left pBdr 4px #134e4a, not a missing bar: {quote_p}"
        );
        assert!(
            p_bdr_left_sz(&quote_p).unwrap_or(0) >= 24,
            "quote bar must be letter 4px (sz 24 eighths), not Word hairline, got {:?}: {quote_p}",
            p_bdr_left_sz(&quote_p)
        );
        assert!(
            quote_p.contains(&format!(r#"w:val="{QUOTE_COLOR}""#))
                && !quote_p.contains(r#"w:color="auto""#)
                && !quote_p.contains(r#"w:color="CCC""#)
                && !quote_p.contains(r#"w:val="nil""#),
            "quote bar must stay letter teal, not Word auto/gray: {quote_p}"
        );
        let quote_style = style_fragment(STYLES_XML, "Quote");
        assert!(
            quote_style.contains("<w:pBdr>")
                && quote_style.contains("<w:left ")
                && quote_style.contains(r#"w:sz="24""#)
                && quote_style.contains(&format!(r#"w:color="{QUOTE_COLOR}""#))
                && quote_style.contains(&format!(r#"<w:color w:val="{QUOTE_COLOR}"/>"#)),
            "Quote style must keep letter 4px #134e4a bar + teal type: {quote_style}"
        );
        let quote_after = hu_to_twips(QUOTE_AFTER_HU);
        assert_eq!(QUOTE_AFTER_HU, 1200);
        assert_eq!(
            quote_after, 240,
            "letter CSS blockquote 1rem is 240 twips, got {quote_after}"
        );
        assert_eq!(
            spacing_attr(&quote_p, "w:after"),
            Some(quote_after),
            "blockquote must space after 1rem, not Word Normal 8pt: {quote_p}"
        );
        assert_eq!(
            spacing_attr(&quote_style, "w:after"),
            Some(quote_after),
            "Quote style after must stay letter 1rem: {quote_style}"
        );
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.quote);
        assert!(p.plain_text().contains("Replay is evidence"));
    }

    #[test]
    fn write_quote_after_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        quote.space_after = 200;
        section.body.push(Block::Paragraph(quote));
        let mut code = Paragraph::from_text("docagent prove examples/letter.md");
        code.code_block = true;
        section.body.push(Block::Paragraph(code));
        let mut table = Table::from_cells(vec![vec!["Hop".into()]]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        section
            .body
            .push(Block::Paragraph(task_item("Receiving dock is clear", true)));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![image_run(mark_png(), 2400, 2400, "Harbor mark")];
        section.body.push(Block::Paragraph(img_p));
        doc.sections.push(section);

        let after = hu_to_twips(QUOTE_AFTER_HU);
        let body_after = hu_to_twips(BODY_AFTER_HU);
        let image_after = hu_to_twips(IMAGE_AFTER_HU);
        assert_eq!(QUOTE_AFTER_HU, 1200);
        assert_eq!(after, 240, "letter CSS blockquote 1rem is 240 twips, got {after}");
        assert!(
            QUOTE_AFTER_HU > BODY_AFTER_HU && after > body_after,
            "quote 1rem must beat body 0.7rem ({body_after})"
        );
        assert!(
            QUOTE_AFTER_HU > IMAGE_AFTER_HU && after > image_after,
            "quote 1rem must stay looser than Harbor after-mark 0.7rem"
        );
        assert_eq!(QUOTE_AFTER_HU, LIST_AFTER_HU);
        assert_eq!(QUOTE_AFTER_HU, TABLE_MARGIN_HU);

        let xml = document_xml(&doc);
        let quote_p = para_containing(&xml, "Replay is evidence");
        assert_eq!(
            spacing_attr(&quote_p, "w:after"),
            Some(after),
            "blockquote must space after 1rem (240 twips), not IR 200 / Word 8pt: {quote_p}"
        );
        assert!(
            !quote_p.contains(r#"w:after="160""#) && !quote_p.contains(r#"w:after="80""#),
            "Word Normal 8pt / jammed IR after the quote vs WeasyPrint 1rem: {quote_p}"
        );
        assert!(
            quote_p.contains(r#"w:val="Quote""#)
                && quote_p.contains("<w:pBdr>")
                && p_bdr_left_sz(&quote_p).unwrap_or(0) >= 24
                && quote_p.contains(&format!(r#"w:color="{QUOTE_COLOR}""#)),
            "quote after 1rem must keep the 4px #134e4a bar: {quote_p}"
        );
        let quote_style = style_fragment(STYLES_XML, "Quote");
        assert_eq!(
            spacing_attr(&quote_style, "w:after"),
            Some(after),
            "Quote style after must stay letter 1rem so style-only Quote is not Normal 8pt: {quote_style}"
        );
        let body_p = para_containing(&xml, "6 September 2026");
        assert_eq!(
            spacing_attr(&body_p, "w:after"),
            Some(body_after),
            "quote 1rem after must not leak onto body: {body_p}"
        );
        let mark_p = para_containing(&xml, "<a:blip r:embed=");
        assert_eq!(
            spacing_attr(&mark_p, "w:after"),
            Some(image_after),
            "quote 1rem after must keep Harbor after-mark 0.7rem: {mark_p}"
        );
        let hop_tc = cell_containing(&xml, "Hop");
        assert!(
            hop_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && hop_tc.contains(&format!(r#"<w:vAlign w:val="{CELL_VALIGN}"/>"#)),
            "quote after must keep th teal fill + valign top: {hop_tc}"
        );
        let dock_p = para_containing(&xml, "Receiving dock is clear");
        assert_eq!(
            num_id_attr(&dock_p),
            Some(TASK_CHECKED_NUM_ID),
            "quote after must keep checked task numId 4: {dock_p}"
        );
        let emu = hu_to_emu(MIN_IMAGE_EDGE_HU);
        assert!(
            xml.contains("<a:blip r:embed=")
                && xml.contains(&format!(r#"<wp:extent cx="{emu}" cy="{emu}"/>"#)),
            "quote after must keep 24pt a:blip: {xml}"
        );
    }

    #[test]
    fn write_code_block_after_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        let mut code = Paragraph::from_text("docagent prove examples/letter.md");
        code.code_block = true;
        code.space_after = 200;
        section.body.push(Block::Paragraph(code));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("Agent replay:")));
        let mut table = Table::from_cells(vec![vec!["Hop".into()]]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        section
            .body
            .push(Block::Paragraph(task_item("Receiving dock is clear", true)));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![image_run(mark_png(), 2400, 2400, "Harbor mark")];
        section.body.push(Block::Paragraph(img_p));
        doc.sections.push(section);

        let after = hu_to_twips(CODE_AFTER_HU);
        let body_after = hu_to_twips(BODY_AFTER_HU);
        let image_after = hu_to_twips(IMAGE_AFTER_HU);
        let image_before = hu_to_twips(IMAGE_BEFORE_HU);
        assert_eq!(CODE_AFTER_HU, 1200);
        assert_eq!(after, 240, "letter CSS pre 1rem is 240 twips, got {after}");
        assert!(
            CODE_AFTER_HU > BODY_AFTER_HU && after > body_after,
            "code 1rem must beat body 0.7rem ({body_after})"
        );
        assert!(
            CODE_AFTER_HU > IMAGE_AFTER_HU && after > image_after,
            "code 1rem must stay looser than Harbor after-mark 0.7rem"
        );
        assert_eq!(CODE_AFTER_HU, QUOTE_AFTER_HU);
        assert_eq!(CODE_AFTER_HU, LIST_AFTER_HU);
        assert_eq!(CODE_AFTER_HU, TABLE_MARGIN_HU);
        assert_eq!(CODE_BEFORE_HU, 720);
        assert_eq!(CODE_BEFORE_HU, IMAGE_BEFORE_HU);
        assert_eq!(IMAGE_BEFORE_HU, 720);
        assert_eq!(IMAGE_AFTER_HU, 840);

        let xml = document_xml(&doc);
        let code_p = para_containing(&xml, "docagent prove");
        assert_eq!(
            spacing_attr(&code_p, "w:after"),
            Some(after),
            "code_block must space after 1rem (240 twips), not IR 200 / Word 8pt: {code_p}"
        );
        assert_eq!(
            spacing_attr(&code_p, "w:before"),
            Some(hu_to_twips(CODE_BEFORE_HU)),
            "code after 1rem must keep letter pre 0.6rem before: {code_p}"
        );
        assert!(
            !code_p.contains(r#"w:after="160""#)
                && !code_p.contains(r#"w:after="80""#)
                && !code_p.contains(r#"w:after="168""#),
            "Word Normal 8pt / jammed IR / Harbor 0.7rem after the prove fence vs WeasyPrint 1rem: {code_p}"
        );
        assert!(
            code_p.contains(r#"w:val="CodeBlock""#)
                && code_p.contains(&format!(r#"w:fill="{CODE_FILL}""#)),
            "code after 1rem must keep the teal Consolas wash: {code_p}"
        );
        let y_sz = hu_to_border_sz(CODE_PAD_Y_HU);
        assert!(
            code_p.contains("<w:pBdr>")
                && p_bdr_top_sz(&code_p) == Some(y_sz)
                && p_bdr_bottom_sz(&code_p) == Some(y_sz)
                && code_p.contains(&format!(r#"w:color="{CODE_FILL}""#)),
            "code after 1rem must keep letter pre 0.75rem Y pad: {code_p}"
        );
        let pad = hu_to_twips(CODE_PAD_X_HU);
        assert_eq!(
            (ind_attr(&code_p, "w:left"), ind_attr(&code_p, "w:right")),
            (Some(pad), Some(pad)),
            "code after 1rem must keep letter pre 1rem X pad: {code_p}"
        );
        let code_style = style_fragment(STYLES_XML, "CodeBlock");
        assert_eq!(
            spacing_attr(&code_style, "w:after"),
            Some(after),
            "CodeBlock style after must stay letter 1rem so style-only CodeBlock is not Normal 8pt: {code_style}"
        );
        assert!(
            code_style.contains("<w:pBdr>")
                && p_bdr_top_sz(&code_style) == Some(y_sz)
                && p_bdr_bottom_sz(&code_style) == Some(y_sz),
            "code after must keep CodeBlock style 0.75rem Y pad: {code_style}"
        );
        let quote_p = para_containing(&xml, "Replay is evidence");
        assert_eq!(
            spacing_attr(&quote_p, "w:after"),
            Some(hu_to_twips(QUOTE_AFTER_HU)),
            "code 1rem after must keep quote 1rem: {quote_p}"
        );
        assert!(
            quote_p.contains("<w:pBdr>")
                && p_bdr_left_sz(&quote_p).unwrap_or(0) >= 24
                && quote_p.contains(&format!(r#"w:color="{QUOTE_COLOR}""#)),
            "code after 1rem must keep the 4px #134e4a bar: {quote_p}"
        );
        let body_p = para_containing(&xml, "6 September 2026");
        assert_eq!(
            spacing_attr(&body_p, "w:after"),
            Some(body_after),
            "code 1rem after must not leak onto body: {body_p}"
        );
        let mark_p = para_containing(&xml, "<a:blip r:embed=");
        assert_eq!(
            spacing_attr(&mark_p, "w:before"),
            Some(image_before),
            "code after must keep Harbor before-mark 0.6rem: {mark_p}"
        );
        assert_eq!(
            spacing_attr(&mark_p, "w:after"),
            Some(image_after),
            "code 1rem after must keep Harbor after-mark 0.7rem: {mark_p}"
        );
        let hop_tc = cell_containing(&xml, "Hop");
        assert!(
            hop_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && hop_tc.contains(&format!(r#"<w:vAlign w:val="{CELL_VALIGN}"/>"#)),
            "code after must keep th teal fill + valign top: {hop_tc}"
        );
        let dock_p = para_containing(&xml, "Receiving dock is clear");
        assert_eq!(
            num_id_attr(&dock_p),
            Some(TASK_CHECKED_NUM_ID),
            "code after must keep checked task numId 4: {dock_p}"
        );
        let emu = hu_to_emu(MIN_IMAGE_EDGE_HU);
        assert!(
            xml.contains("<a:blip r:embed=")
                && xml.contains(&format!(r#"<wp:extent cx="{emu}" cy="{emu}"/>"#)),
            "code after must keep 24pt a:blip: {xml}"
        );
    }

    #[test]
    fn write_code_block_before_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        let mut code = Paragraph::from_text("docagent prove examples/letter.md");
        code.code_block = true;
        code.space_before = 80;
        code.space_after = 200;
        section.body.push(Block::Paragraph(code));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("Agent replay:")));
        let mut table = Table::from_cells(vec![vec!["Hop".into()]]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        section
            .body
            .push(Block::Paragraph(task_item("Receiving dock is clear", true)));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![image_run(mark_png(), 2400, 2400, "Harbor mark")];
        section.body.push(Block::Paragraph(img_p));
        doc.sections.push(section);

        let before = hu_to_twips(CODE_BEFORE_HU);
        let after = hu_to_twips(CODE_AFTER_HU);
        let body_after = hu_to_twips(BODY_AFTER_HU);
        let image_after = hu_to_twips(IMAGE_AFTER_HU);
        let image_before = hu_to_twips(IMAGE_BEFORE_HU);
        let quote_before = hu_to_twips(QUOTE_BEFORE_HU);
        assert_eq!(CODE_BEFORE_HU, 720);
        assert_eq!(before, 144, "letter CSS pre 0.6rem is 144 twips, got {before}");
        assert_eq!(CODE_BEFORE_HU, IMAGE_BEFORE_HU);
        assert!(
            CODE_BEFORE_HU > QUOTE_BEFORE_HU && before > quote_before,
            "pre 0.6rem must beat quote 0.5rem ({quote_before})"
        );
        assert!(
            CODE_AFTER_HU > CODE_BEFORE_HU && after > before,
            "pre 0.6rem before must stay tighter than 1rem after"
        );
        assert!(
            CODE_BEFORE_HU < BODY_AFTER_HU && before < body_after,
            "pre 0.6rem must stay tighter than body 0.7rem ({body_after})"
        );
        assert_eq!(CODE_AFTER_HU, 1200);
        assert_eq!(IMAGE_BEFORE_HU, 720);
        assert_eq!(IMAGE_AFTER_HU, 840);
        assert_eq!(QUOTE_AFTER_HU, 1200);

        let xml = document_xml(&doc);
        let code_p = para_containing(&xml, "docagent prove");
        assert_eq!(
            spacing_attr(&code_p, "w:before"),
            Some(before),
            "code_block must space before 0.6rem (144 twips), not IR 80 / Word 6pt: {code_p}"
        );
        assert!(
            !code_p.contains(r#"w:before="16""#)
                && !code_p.contains(r#"w:before="80""#)
                && !code_p.contains(r#"w:before="120""#)
                && !code_p.contains(r#"w:before="160""#),
            "IR 80 / Word 6pt / Normal 8pt before the prove fence vs WeasyPrint 0.6rem: {code_p}"
        );
        assert_eq!(
            spacing_attr(&code_p, "w:after"),
            Some(after),
            "code 0.6rem before must keep letter.css 1rem after: {code_p}"
        );
        assert!(
            code_p.contains(r#"w:val="CodeBlock""#)
                && code_p.contains(&format!(r#"w:fill="{CODE_FILL}""#)),
            "code before 0.6rem must keep the teal Consolas wash: {code_p}"
        );
        let y_sz = hu_to_border_sz(CODE_PAD_Y_HU);
        assert!(
            code_p.contains("<w:pBdr>")
                && p_bdr_top_sz(&code_p) == Some(y_sz)
                && p_bdr_bottom_sz(&code_p) == Some(y_sz)
                && code_p.contains(&format!(r#"w:color="{CODE_FILL}""#)),
            "code before 0.6rem must keep letter pre 0.75rem Y pad: {code_p}"
        );
        let pad = hu_to_twips(CODE_PAD_X_HU);
        assert_eq!(
            (ind_attr(&code_p, "w:left"), ind_attr(&code_p, "w:right")),
            (Some(pad), Some(pad)),
            "code before 0.6rem must keep letter pre 1rem X pad: {code_p}"
        );
        let code_style = style_fragment(STYLES_XML, "CodeBlock");
        assert_eq!(
            spacing_attr(&code_style, "w:before"),
            Some(before),
            "CodeBlock style before must stay letter.css 0.6rem so style-only CodeBlock is not Word 6pt: {code_style}"
        );
        assert_eq!(
            spacing_attr(&code_style, "w:after"),
            Some(after),
            "code before must keep CodeBlock style 1rem after: {code_style}"
        );
        let quote_p = para_containing(&xml, "Replay is evidence");
        assert_eq!(
            spacing_attr(&quote_p, "w:after"),
            Some(hu_to_twips(QUOTE_AFTER_HU)),
            "code 0.6rem before must keep quote 1rem: {quote_p}"
        );
        assert!(
            quote_p.contains("<w:pBdr>")
                && p_bdr_left_sz(&quote_p).unwrap_or(0) >= 24
                && quote_p.contains(&format!(r#"w:color="{QUOTE_COLOR}""#)),
            "code before 0.6rem must keep the 4px #134e4a bar: {quote_p}"
        );
        let body_p = para_containing(&xml, "6 September 2026");
        assert_eq!(
            spacing_attr(&body_p, "w:after"),
            Some(body_after),
            "code 0.6rem before must not leak onto body: {body_p}"
        );
        let mark_p = para_containing(&xml, "<a:blip r:embed=");
        assert_eq!(
            spacing_attr(&mark_p, "w:before"),
            Some(image_before),
            "code before must keep Harbor before-mark 0.6rem: {mark_p}"
        );
        assert_eq!(
            spacing_attr(&mark_p, "w:after"),
            Some(image_after),
            "code 0.6rem before must keep Harbor after-mark 0.7rem: {mark_p}"
        );
        let hop_tc = cell_containing(&xml, "Hop");
        assert!(
            hop_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && hop_tc.contains(&format!(r#"<w:vAlign w:val="{CELL_VALIGN}"/>"#)),
            "code before must keep th teal fill + valign top: {hop_tc}"
        );
        let dock_p = para_containing(&xml, "Receiving dock is clear");
        assert_eq!(
            num_id_attr(&dock_p),
            Some(TASK_CHECKED_NUM_ID),
            "code before must keep checked task numId 4: {dock_p}"
        );
        let emu = hu_to_emu(MIN_IMAGE_EDGE_HU);
        assert!(
            xml.contains("<a:blip r:embed=")
                && xml.contains(&format!(r#"<wp:extent cx="{emu}" cy="{emu}"/>"#)),
            "code before must keep 24pt a:blip: {xml}"
        );
    }

    #[test]
    fn write_body_after_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut date = Paragraph::from_text("6 September 2026");
        date.space_after = 200;
        section.body.push(Block::Paragraph(date));
        let mut dest = Paragraph::from_text("To: operations desk, Duisburg terminal.");
        dest.space_after = 200;
        section.body.push(Block::Paragraph(dest));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        let mut code = Paragraph::from_text("docagent prove examples/letter.md");
        code.code_block = true;
        code.space_before = 80;
        section.body.push(Block::Paragraph(code));
        let mut table = Table::from_cells(vec![vec!["Hop".into()]]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        section.body.push(Block::Paragraph(numbered_item(
            "Receiving dock is clear",
            0,
            NumberFormat::Task,
        )));
        section.body.push(Block::Paragraph(numbered_item(
            "Bay 4, H2O seals intact",
            1,
            NumberFormat::Bullet,
        )));
        section.body.push(Block::Paragraph(numbered_item(
            "Replay the convert instead of hoping",
            0,
            NumberFormat::Task,
        )));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![image_run(mark_png(), 2400, 2400, "Harbor mark")];
        section.body.push(Block::Paragraph(img_p));
        doc.sections.push(section);

        let after = hu_to_twips(BODY_AFTER_HU);
        let image_after = hu_to_twips(IMAGE_AFTER_HU);
        let image_before = hu_to_twips(IMAGE_BEFORE_HU);
        let list_after = hu_to_twips(LIST_AFTER_HU);
        let nested_gap = hu_to_twips(LIST_NESTED_GAP_HU);
        assert_eq!(BODY_AFTER_HU, 840);
        assert_eq!(after, 168, "letter CSS p 0.7rem is 168 twips, got {after}");
        assert_eq!(IMAGE_AFTER_HU, 840);
        assert_eq!(
            BODY_AFTER_HU, IMAGE_AFTER_HU,
            "body 0.7rem after must stay the same strut as collapsed p+img"
        );
        assert!(
            BODY_AFTER_HU > HEADING1_AFTER_HU && after > hu_to_twips(HEADING1_AFTER_HU),
            "body 0.7rem must stay looser than h1 0.6rem"
        );
        assert!(
            LIST_AFTER_HU > BODY_AFTER_HU && list_after > after,
            "last-list 1rem must stay looser than body 0.7rem"
        );
        assert_eq!(CODE_BEFORE_HU, 720);
        assert_eq!(QUOTE_AFTER_HU, 1200);
        assert_eq!(IMAGE_BEFORE_HU, 720);

        let xml = document_xml(&doc);
        let body_p = para_containing(&xml, "6 September 2026");
        let dest_p = para_containing(&xml, "operations desk");
        assert_eq!(
            spacing_attr(&body_p, "w:after"),
            Some(after),
            "body must space after 0.7rem (168 twips), not IR 200 / Word 8pt: {body_p}"
        );
        assert_eq!(
            spacing_attr(&dest_p, "w:after"),
            Some(after),
            "consecutive body must keep letter.css 0.7rem, not Word 8pt: {dest_p}"
        );
        assert!(
            !body_p.contains(r#"w:after="160""#)
                && !body_p.contains(r#"w:after="40""#)
                && !body_p.contains(r#"w:after="240""#),
            "Word Normal 8pt / jammed IR / last-list 1rem after body vs WeasyPrint 0.7rem: {body_p}"
        );
        assert_eq!(
            spacing_attr(&body_p, "w:line"),
            Some(BODY_LINE),
            "body 0.7rem after must keep Word 1.15: {body_p}"
        );
        let normal_style = style_fragment(STYLES_XML, "Normal");
        assert_eq!(
            spacing_attr(&normal_style, "w:after"),
            Some(after),
            "Normal style after must stay letter.css 0.7rem so style-only Normal is not Word 8pt: {normal_style}"
        );
        assert!(
            STYLES_XML.contains(r#"<w:pPrDefault><w:pPr><w:spacing w:after="168" w:line="276""#),
            "docDefaults after must stay letter.css 0.7rem, not Word 8pt"
        );
        let quote_p = para_containing(&xml, "Replay is evidence");
        assert_eq!(
            spacing_attr(&quote_p, "w:after"),
            Some(hu_to_twips(QUOTE_AFTER_HU)),
            "body 0.7rem after must keep quote 1rem: {quote_p}"
        );
        assert!(
            quote_p.contains("<w:pBdr>")
                && p_bdr_left_sz(&quote_p).unwrap_or(0) >= 24
                && quote_p.contains(&format!(r#"w:color="{QUOTE_COLOR}""#)),
            "body 0.7rem after must keep the 4px #134e4a bar: {quote_p}"
        );
        let code_p = para_containing(&xml, "docagent prove");
        assert_eq!(
            spacing_attr(&code_p, "w:before"),
            Some(hu_to_twips(CODE_BEFORE_HU)),
            "body 0.7rem after must keep pre 0.6rem before: {code_p}"
        );
        let mark_p = para_containing(&xml, "<a:blip r:embed=");
        assert_eq!(
            spacing_attr(&mark_p, "w:before"),
            Some(image_before),
            "body 0.7rem after must keep Harbor before-mark 0.6rem: {mark_p}"
        );
        assert_eq!(
            spacing_attr(&mark_p, "w:after"),
            Some(image_after),
            "body 0.7rem after must keep Harbor after-mark 0.7rem: {mark_p}"
        );
        let dock_p = para_containing(&xml, "Receiving dock is clear");
        let last_task = para_containing(&xml, "Replay the convert instead of hoping");
        assert_eq!(
            spacing_attr(&dock_p, "w:after"),
            Some(nested_gap),
            "body 0.7rem after must keep nested 0.25rem: {dock_p}"
        );
        assert_eq!(
            spacing_attr(&last_task, "w:after"),
            Some(list_after),
            "body 0.7rem after must keep last-list 1rem: {last_task}"
        );
        let hop_tc = cell_containing(&xml, "Hop");
        assert!(
            hop_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && hop_tc.contains(&format!(r#"<w:vAlign w:val="{CELL_VALIGN}"/>"#)),
            "body 0.7rem after must keep th teal fill + valign top: {hop_tc}"
        );
        let emu = hu_to_emu(MIN_IMAGE_EDGE_HU);
        assert!(
            xml.contains("<a:blip r:embed=")
                && xml.contains(&format!(r#"<wp:extent cx="{emu}" cy="{emu}"/>"#)),
            "body 0.7rem after must keep 24pt a:blip: {xml}"
        );
    }

    #[test]
    fn write_body_run_color_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        let mut super_p = Paragraph::from_text("A");
        super_p.runs[0].style.superscript = true;
        section.body.push(Block::Paragraph(super_p));
        let mut sub_p = Paragraph::from_text("2");
        sub_p.runs[0].style.subscript = true;
        section.body.push(Block::Paragraph(sub_p));
        let mut table = Table::from_cells(vec![
            vec!["Hop".into()],
            vec!["Input".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        section
            .body
            .push(Block::Paragraph(task_item("Receiving dock is clear", true)));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![image_run(mark_png(), 2400, 2400, "Harbor mark")];
        section.body.push(Block::Paragraph(img_p));
        doc.sections.push(section);

        let xml = document_xml(&doc);
        let body_p = para_containing(&xml, "6 September 2026");
        let body_run = run_containing(&body_p, ">6 September 2026</w:t>");
        assert!(
            body_run.contains(&format!(r#"<w:color w:val="{BODY_COLOR}"/>"#))
                && !body_run.contains(r#"w:val="000000""#)
                && !body_run.contains(r#"w:color="auto""#),
            "body run must be letter #134e4a, not UA black: {body_run}"
        );
        let normal = style_fragment(STYLES_XML, "Normal");
        assert!(
            normal.contains(&format!(r#"<w:color w:val="{BODY_COLOR}"/>"#))
                && !normal.contains(r#"w:val="000000""#),
            "Normal must keep letter teal so style-only body is not UA black: {normal}"
        );
        assert!(
            spacing_attr(&normal, "w:line").unwrap_or(0) >= BODY_LINE
                && normal.contains(r#"w:lineRule="auto""#),
            "Normal line must stay Word 1.15 ({BODY_LINE}): {normal}"
        );
        assert!(
            spacing_attr(&body_p, "w:line").unwrap_or(0) >= BODY_LINE,
            "body line must stay Word 1.15 ({BODY_LINE}): {body_p}"
        );
        assert_eq!(BODY_COLOR, QUOTE_COLOR);
        assert_eq!(BODY_COLOR, TH_COLOR);

        let quote_p = para_containing(&xml, "Replay is evidence");
        assert!(
            ind_attr(&quote_p, "w:left").unwrap_or(0) >= QUOTE_LEFT_TWIPS,
            "body color must keep Quote 0.5in indent: {quote_p}"
        );
        assert!(
            quote_p.contains("<w:pBdr>")
                && quote_p.contains("<w:left ")
                && p_bdr_left_sz(&quote_p).unwrap_or(0) >= 24
                && quote_p.contains(&format!(r#"w:color="{QUOTE_COLOR}""#)),
            "body color must keep quote 4px #134e4a left bar: {quote_p}"
        );
        let a_run = run_containing(&xml, ">A</w:t>");
        let two_run = run_containing(&xml, ">2</w:t>");
        assert!(
            a_run.contains(r#"<w:vertAlign w:val="superscript"/>"#)
                && two_run.contains(r#"<w:vertAlign w:val="subscript"/>"#),
            "body color must keep super/sub vertAlign: {a_run} {two_run}"
        );
        let hop_tc = cell_containing(&xml, "Hop");
        assert!(
            hop_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && hop_tc.contains(&format!(r#"w:color="{TH_RULE_COLOR}""#))
                && hop_tc.contains("<w:tcBorders>"),
            "body color must keep th teal fill: {hop_tc}"
        );
        let grid = tbl_borders_block(&xml);
        assert!(
            grid.contains(&format!(r#"w:color="{TBL_BORDER_COLOR}""#))
                && hu_to_border_sz(TBL_BORDER_HU) >= 6
                && !grid.contains(r#"w:color="000000""#),
            "body color must keep letter 1px #cbd5e1 hop grid: {grid}"
        );
        let dock_p = para_containing(&xml, "Receiving dock is clear");
        assert_eq!(
            num_id_in(&dock_p),
            Some(TASK_CHECKED_NUM_ID),
            "body color must keep checked task numId 4: {dock_p}"
        );
        assert!(
            dock_p.contains(&format!(r#"<w:color w:val="{BODY_COLOR}"/>"#)),
            "task item type must stay letter teal: {dock_p}"
        );
        let (cx, cy) = {
            let start = xml.find("<wp:extent ").expect("wp:extent");
            let tag = xml[start..].split('>').next().expect("extent tag");
            let cx = tag
                .split(r#"cx=""#)
                .nth(1)
                .and_then(|s| s.split('"').next())
                .and_then(|s| s.parse::<i64>().ok())
                .expect("cx");
            let cy = tag
                .split(r#"cy=""#)
                .nth(1)
                .and_then(|s| s.split('"').next())
                .and_then(|s| s.parse::<i64>().ok())
                .expect("cy");
            (cx, cy)
        };
        let min_emu = hu_to_emu(MIN_IMAGE_EDGE_HU);
        assert!(
            cx >= min_emu && cy >= min_emu && xml.contains("<a:blip r:embed="),
            "body color must keep 24pt a:blip, got {cx}×{cy}: {xml}"
        );
    }

    #[test]
    fn write_body_font_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation");
        h2.outline_level = Some(2);
        h2.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h2));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        let mut code = Paragraph::from_text("docagent prove examples/letter.md");
        code.code_block = true;
        section.body.push(Block::Paragraph(code));
        let mut table = Table::from_cells(vec![
            vec!["Hop".into()],
            vec!["Input".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        section
            .body
            .push(Block::Paragraph(task_item("Receiving dock is clear", true)));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![image_run(mark_png(), 2400, 2400, "Harbor mark")];
        section.body.push(Block::Paragraph(img_p));
        let mut link = Paragraph::from_text("");
        link.runs.push(Run::hyperlink(
            "DocAgent",
            "https://github.com/kevin9327/docagent",
        ));
        section.body.push(Block::Paragraph(link));
        doc.sections.push(section);

        let xml = document_xml(&doc);
        assert_eq!(BODY_FONT, "Segoe UI");
        let body_p = para_containing(&xml, "6 September 2026");
        let body_run = run_containing(&body_p, ">6 September 2026</w:t>");
        assert!(
            body_run.contains(&format!(r#"w:ascii="{BODY_FONT}""#))
                && body_run.contains(&format!(r#"w:hAnsi="{BODY_FONT}""#))
                && !body_run.contains("Calibri"),
            "body run must be letter Segoe UI, not Word theme Calibri: {body_run}"
        );
        let h1_run = run_containing(&xml, ">Northwind Freight</w:t>");
        let h2_run = run_containing(&xml, ">Delivery confirmation</w:t>");
        assert!(
            h1_run.contains(&format!(r#"w:ascii="{BODY_FONT}""#))
                && h2_run.contains(&format!(r#"w:ascii="{BODY_FONT}""#))
                && !h1_run.contains("Calibri")
                && !h2_run.contains("Calibri"),
            "H1/H2 runs must stay letter Segoe UI, not Calibri: {h1_run} {h2_run}"
        );
        let normal = style_fragment(STYLES_XML, "Normal");
        let h1_style = style_fragment(STYLES_XML, "Heading1");
        let h2_style = style_fragment(STYLES_XML, "Heading2");
        let h3_style = style_fragment(STYLES_XML, "Heading3");
        let quote_style = style_fragment(STYLES_XML, "Quote");
        let list_style = style_fragment(STYLES_XML, "ListParagraph");
        let link_style = style_fragment(STYLES_XML, "Hyperlink");
        for (name, style) in [
            ("Normal", normal.as_str()),
            ("Heading1", h1_style.as_str()),
            ("Heading2", h2_style.as_str()),
            ("Heading3", h3_style.as_str()),
            ("Quote", quote_style.as_str()),
            ("ListParagraph", list_style.as_str()),
            ("Hyperlink", link_style.as_str()),
        ] {
            assert!(
                style.contains(&format!(r#"w:ascii="{BODY_FONT}""#))
                    && style.contains(&format!(r#"w:hAnsi="{BODY_FONT}""#))
                    && !style.contains("Calibri"),
                "{name} must keep letter Segoe UI, not theme Calibri: {style}"
            );
        }
        let quote_p = para_containing(&xml, "Replay is evidence");
        assert!(
            quote_p.contains(&format!(r#"w:ascii="{BODY_FONT}""#))
                && quote_p.contains("<w:pBdr>")
                && p_bdr_left_sz(&quote_p).unwrap_or(0) >= 24,
            "quote must keep Segoe UI + 4px bar: {quote_p}"
        );
        let code_p = para_containing(&xml, "docagent prove");
        assert!(
            code_p.contains("Consolas") && !code_p.contains(&format!(r#"w:ascii="{BODY_FONT}""#)),
            "code_block must keep Consolas, not body Segoe UI: {code_p}"
        );
        let hop_p = para_containing(&xml, "Hop");
        assert!(
            hop_p.contains(&format!(r#"w:ascii="{BODY_FONT}""#)) && hop_p.contains("<w:b/>"),
            "th type must stay letter Segoe UI bold: {hop_p}"
        );
        let dock_p = para_containing(&xml, "Receiving dock is clear");
        assert!(
            dock_p.contains(&format!(r#"w:ascii="{BODY_FONT}""#)),
            "task item type must stay letter Segoe UI: {dock_p}"
        );
        let link_p = para_containing(&xml, "DocAgent");
        assert!(
            link_p.contains(&format!(r#"w:ascii="{BODY_FONT}""#))
                && link_p.contains(r#"w:val="0D9488""#),
            "hyperlink must stay letter Segoe UI teal: {link_p}"
        );
        let hop_tc = cell_containing(&xml, "Hop");
        assert!(
            hop_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && hop_tc.contains(&format!(r#"w:color="{TH_RULE_COLOR}""#)),
            "body font must keep th teal fill: {hop_tc}"
        );
        let min_emu = hu_to_emu(MIN_IMAGE_EDGE_HU);
        assert!(
            xml.contains("<a:blip r:embed=")
                && xml.contains(&format!(r#"<wp:extent cx="{min_emu}" cy="{min_emu}"/>"#)),
            "body font must keep 24pt a:blip: {xml}"
        );
    }

    #[test]
    fn roundtrip_keeps_superscript() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("A");
        p.runs[0].style.superscript = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = document_xml(&doc);
        let a_run = run_containing(&xml, ">A</w:t>");
        assert!(
            a_run.contains(r#"<w:rStyle w:val="Superscript"/>"#)
                && a_run.contains(r#"<w:vertAlign w:val="superscript"/>"#)
                && a_run.contains(">A</w:t>"),
            "PDF/^A^ must emit Superscript + w:vertAlign so Word raises the A, not missing: {a_run}"
        );
        assert!(
            !a_run.contains(r#"w:val="baseline""#) && !a_run.contains(r#"w:val="subscript""#),
            "superscript must not reset to baseline or subscript: {a_run}"
        );
        let super_style = style_fragment(STYLES_XML, "Superscript");
        assert!(
            super_style.contains(r#"<w:vertAlign w:val="superscript"/>"#)
                && !super_style.contains(r#"w:val="baseline""#)
                && !super_style.contains(r#"w:val="subscript""#),
            "Superscript style must be Word Font Effects Super, not missing: {super_style}"
        );
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.superscript
                && !r.style.subscript
                && matches!(&r.content, RunContent::Text(t) if t == "A")
        }));
    }

    #[test]
    fn roundtrip_keeps_subscript() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("2");
        p.runs[0].style.subscript = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = document_xml(&doc);
        let two_run = run_containing(&xml, ">2</w:t>");
        assert!(
            two_run.contains(r#"<w:rStyle w:val="Subscript"/>"#)
                && two_run.contains(r#"<w:vertAlign w:val="subscript"/>"#)
                && two_run.contains(">2</w:t>"),
            "H~2~O must emit Subscript + w:vertAlign so Word lowers the 2, not missing: {two_run}"
        );
        assert!(
            !two_run.contains(r#"w:val="baseline""#) && !two_run.contains(r#"w:val="superscript""#),
            "subscript must not reset to baseline or superscript: {two_run}"
        );
        let sub_style = style_fragment(STYLES_XML, "Subscript");
        assert!(
            sub_style.contains(r#"<w:vertAlign w:val="subscript"/>"#)
                && !sub_style.contains(r#"w:val="baseline""#)
                && !sub_style.contains(r#"w:val="superscript""#),
            "Subscript style must be Word Font Effects Sub, not missing: {sub_style}"
        );
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.subscript
                && !r.style.superscript
                && matches!(&r.content, RunContent::Text(t) if t == "2")
        }));
    }

    #[test]
    fn roundtrip_keeps_underline() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("09:00 local");
        p.runs[0].style.underline = Underline::Single;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = document_xml(&doc);
        let under_p = para_containing(&xml, "09:00 local");
        assert!(
            under_p.contains(r#"w:val="Underline""#)
                && under_p.contains(r#"<w:u w:val="single"/>"#)
                && under_p.contains(">09:00 local</w:t>"),
            "__09:00 local__ must emit Underline + w:u so Word paints the rule, not missing: {under_p}"
        );
        let under_style = style_fragment(STYLES_XML, "Underline");
        assert!(
            under_style.contains(r#"<w:u w:val="single"/>"#)
                && !under_style.contains(r#"w:val="none""#),
            "Underline style must be Word Single, not a missing rule: {under_style}"
        );
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.underline != Underline::None
                && matches!(&r.content, RunContent::Text(t) if t == "09:00 local")
        }));
    }

    #[test]
    fn roundtrip_keeps_highlight() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("hashes");
        p.runs[0].style.highlight = Some(docagent_model::Color::MARK);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = document_xml(&doc);
        assert!(xml.contains(r#"w:val="yellow""#), "{xml}");
        assert!(xml.contains(r#"w:val="Mark""#), "{xml}");
        assert!(
            xml.contains(&format!(r#"w:fill="{MARK_FILL}""#)),
            "mark must keep letter amber shading so the wash is visible: {xml}"
        );
        let mark_p = para_containing(&xml, "hashes");
        let mark_run = run_containing(&mark_p, ">hashes</w:t>");
        assert!(
            mark_run.contains(&format!(r#"<w:color w:val="{MARK_COLOR}"/>"#))
                && !mark_run.contains(r#"w:val="000000""#)
                && !mark_run.contains(r#"w:color="auto""#),
            "mark run must stay letter.html #134e4a, not highlighter black: {mark_run}"
        );
        assert!(
            bdr_space_for_color(&mark_p, MARK_FILL).unwrap_or(0) >= MARK_BDR_SPACE_PT,
            "mark must keep letter .05em/.2em Word rPr padding, not a tight glyph wash: {mark_p}"
        );
        let mark_style = style_fragment(STYLES_XML, "Mark");
        assert!(
            mark_style.contains(r#"w:val="yellow""#)
                && mark_style.contains(&format!(r#"w:fill="{MARK_FILL}""#))
                && mark_style.contains(&format!(r#"<w:color w:val="{MARK_COLOR}"/>"#))
                && !mark_style.contains(r#"w:val="000000""#),
            "Mark style must stay Word highlight + letter teal on amber: {mark_style}"
        );
        assert!(
            bdr_space_for_color(&mark_style, MARK_FILL).unwrap_or(0) >= MARK_BDR_SPACE_PT,
            "Mark style must keep letter pad: {mark_style}"
        );
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.highlight.is_some() && matches!(&r.content, RunContent::Text(t) if t == "hashes")
        }));
    }

    fn task_item(text: &str, checked: bool) -> Paragraph {
        let mut p = Paragraph::from_text(text);
        p.numbering = Some(NumberingRef {
            definition_id: 2,
            level: 0,
            start: checked.then_some(1),
            format: Some(NumberFormat::Task),
        });
        p
    }

    fn num_id_in(fragment: &str) -> Option<u32> {
        let start = fragment.find("<w:numId ")?;
        let tag = fragment[start..].split('>').next()?;
        let needle = r#"w:val=""#;
        tag.split(needle).nth(1)?.split('"').next()?.parse().ok()
    }

    #[test]
    fn roundtrip_keeps_task_item() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(task_item("Replay the convert", true)));
        doc.sections.push(section);
        let xml = document_xml(&doc);
        let pxml = para_containing(&xml, "Replay the convert");
        assert_eq!(
            num_id_in(&pxml),
            Some(TASK_CHECKED_NUM_ID),
            "checked - [x] must use the Word checkbox numId, not a disc: {pxml}"
        );
        assert!(
            !pxml.contains("[x]") && !pxml.contains("[ ]"),
            "ASCII [x] is not a Word checkbox: {pxml}"
        );
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert_eq!(
            p.numbering.as_ref().map(|n| (n.format, n.start)),
            Some((Some(NumberFormat::Task), Some(1)))
        );
        assert_eq!(p.plain_text(), "Replay the convert");
    }

    #[test]
    fn task_lists_emit_word_checkboxes_not_ascii_boxes() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(task_item("Receiving dock is clear", true)));
        section
            .body
            .push(Block::Paragraph(task_item("Replay the convert", false)));
        doc.sections.push(section);

        let xml = document_xml(&doc);
        let dock = para_containing(&xml, "Receiving dock is clear");
        let replay = para_containing(&xml, "Replay the convert");
        assert_eq!(
            num_id_in(&dock),
            Some(TASK_CHECKED_NUM_ID),
            "checked task must not share the disc numId: {dock}"
        );
        assert_eq!(
            num_id_in(&replay),
            Some(TASK_UNCHECKED_NUM_ID),
            "unchecked task must be a hollow Word box, not [ ]: {replay}"
        );
        assert_ne!(
            num_id_in(&dock),
            num_id_in(&replay),
            "- [x] and - [ ] must use different Word checkbox markers"
        );
        assert!(
            !xml.contains("[x]") && !xml.contains("[ ]"),
            "GFM task markers must not leak into Word: {xml}"
        );
        assert!(
            dock.contains(r#"w:val="ListParagraph""#),
            "tasks stay Word lists: {dock}"
        );

        let numbering = numbering_xml();
        assert!(
            numbering.contains(TASK_UNCHECKED_GLYPH)
                && numbering.contains(TASK_CHECKED_GLYPH),
            "numbering.xml must carry Word ☐/☒ checkbox bullets: {numbering}"
        );
        assert!(
            numbering.contains("Segoe UI Symbol") && numbering.contains("MS Gothic"),
            "Word paints ☐/☒ from Segoe UI Symbol / MS Gothic, not a missing glyph: {numbering}"
        );
        assert!(
            numbering.contains(r#"<w:color w:val="134E4A"/>"#),
            "task checkboxes must stay letter teal: {numbering}"
        );
        assert!(
            numbering.contains(&format!(r#"<w:sz w:val="{TASK_BOX_SZ}"/>"#))
                && numbering.contains(&format!(r#"<w:szCs w:val="{TASK_BOX_SZ}"/>"#)),
            "task checkboxes must stay letter 12pt (sz 24), not a tofu-scale glyph: {numbering}"
        );
        assert!(
            !numbering.contains("[x]") && !numbering.contains("[ ]"),
            "numbering must not use ASCII boxes: {numbering}"
        );

        let bytes = write(&doc).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut packed = String::new();
        zip.by_name("word/numbering.xml")
            .unwrap()
            .read_to_string(&mut packed)
            .unwrap();
        assert!(
            packed.contains(TASK_UNCHECKED_GLYPH) && packed.contains(TASK_CHECKED_GLYPH),
            "packaged numbering.xml must keep Word checkboxes: {packed}"
        );
        assert!(
            packed.contains("Segoe UI Symbol") && packed.contains(r#"w:numId="3""#),
            "packaged task numbering must stay: {packed}"
        );

        let back = roundtrip(&doc).unwrap();
        let nums: Vec<_> = back.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => {
                    p.numbering.as_ref().map(|n| (p.plain_text(), n.format, n.start))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            nums,
            [
                (
                    "Receiving dock is clear".into(),
                    Some(NumberFormat::Task),
                    Some(1)
                ),
                ("Replay the convert".into(), Some(NumberFormat::Task), None),
            ]
        );
    }

    #[test]
    fn write_task_box_size_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        section
            .body
            .push(Block::Paragraph(task_item("Receiving dock is clear", true)));
        section
            .body
            .push(Block::Paragraph(task_item("Replay the convert", false)));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![image_run(mark_png(), 2400, 2400, "Harbor mark")];
        section.body.push(Block::Paragraph(img_p));
        doc.sections.push(section);

        assert_eq!(
            TASK_BOX_SZ, 24,
            "letter .9em + 1.5pt border is Word 12pt (sz 24)"
        );
        let disc = list_abstract_num(0, "bullet");
        let decimal = list_abstract_num(1, "decimal");
        assert!(
            !disc.contains("<w:sz ") && !decimal.contains("<w:sz "),
            "disc/decimal bullets must keep body size, not the task 12pt: {disc} {decimal}"
        );
        let empty = checkbox_abstract_num(2, TASK_UNCHECKED_GLYPH);
        let done = checkbox_abstract_num(3, TASK_CHECKED_GLYPH);
        for (name, xml) in [("empty", empty.as_str()), ("checked", done.as_str())] {
            assert!(
                xml.contains("Segoe UI Symbol")
                    && xml.contains(&format!(r#"<w:sz w:val="{TASK_BOX_SZ}"/>"#))
                    && xml.contains(&format!(r#"<w:szCs w:val="{TASK_BOX_SZ}"/>"#))
                    && xml.contains(r#"<w:color w:val="134E4A"/>"#),
                "{name} task lvl must keep letter 12pt teal ☐/☒, not a missing sz: {xml}"
            );
            assert!(
                !xml.contains(r#"<w:sz w:val="22"/>"#)
                    && !xml.contains(r#"<w:sz w:val="16"/>"#)
                    && !xml.contains(r#"<w:sz w:val="20"/>"#),
                "{name} task box must not inherit body/code size: {xml}"
            );
        }

        let numbering = numbering_xml();
        assert!(
            numbering.contains(&format!(r#"<w:sz w:val="{TASK_BOX_SZ}"/>"#))
                && numbering.contains(&format!(r#"<w:szCs w:val="{TASK_BOX_SZ}"/>"#))
                && numbering.contains(TASK_UNCHECKED_GLYPH)
                && numbering.contains(TASK_CHECKED_GLYPH),
            "numbering.xml must keep 12pt Word checkboxes: {numbering}"
        );

        let xml = document_xml(&doc);
        let dock = para_containing(&xml, "Receiving dock is clear");
        let replay = para_containing(&xml, "Replay the convert");
        assert_eq!(
            num_id_in(&dock),
            Some(TASK_CHECKED_NUM_ID),
            "task box size must keep checked numId 4: {dock}"
        );
        assert_eq!(
            num_id_in(&replay),
            Some(TASK_UNCHECKED_NUM_ID),
            "task box size must keep unchecked numId 3: {replay}"
        );
        let quote_p = para_containing(&xml, "Replay is evidence");
        assert!(
            quote_p.contains("<w:pBdr>")
                && p_bdr_left_sz(&quote_p).unwrap_or(0) >= 24
                && quote_p.contains(&format!(r#"w:color="{QUOTE_COLOR}""#)),
            "task box size must keep quote 4px #134e4a left bar: {quote_p}"
        );
        let hop_tc = cell_containing(&xml, "Hop");
        assert!(
            hop_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && hop_tc.contains(&format!(r#"w:color="{TH_RULE_COLOR}""#)),
            "task box size must keep th teal fill: {hop_tc}"
        );
        assert!(
            xml.contains("<a:blip r:embed="),
            "task box size must keep 24pt a:blip: {xml}"
        );
        let emu = hu_to_emu(MIN_IMAGE_EDGE_HU);
        assert!(
            xml.contains(&format!(r#"<wp:extent cx="{emu}" cy="{emu}"/>"#)),
            "task box size must keep the 24pt a:blip floor: {xml}"
        );

        let bytes = write(&doc).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut packed = String::new();
        zip.by_name("word/numbering.xml")
            .unwrap()
            .read_to_string(&mut packed)
            .unwrap();
        assert!(
            packed.contains(&format!(r#"<w:sz w:val="{TASK_BOX_SZ}"/>"#))
                && packed.contains(&format!(r#"<w:szCs w:val="{TASK_BOX_SZ}"/>"#))
                && packed.contains("Segoe UI Symbol")
                && packed.contains(TASK_CHECKED_GLYPH),
            "packaged numbering.xml must keep letter 12pt checkboxes: {packed}"
        );
    }

    #[test]
    fn read_promotes_ascii_and_ballot_box_prefixes_to_task() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("[x] Capsule hashes")));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("☐ Hope")));
        doc.sections.push(section);
        let back = roundtrip(&doc).unwrap();
        let got: Vec<_> = back.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => {
                    Some((p.plain_text(), p.numbering.as_ref().map(|n| (n.format, n.start))))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            got,
            [
                (
                    "Capsule hashes".into(),
                    Some((Some(NumberFormat::Task), Some(1)))
                ),
                ("Hope".into(), Some((Some(NumberFormat::Task), None))),
            ]
        );
    }

    #[test]
    fn roundtrip_keeps_code_block() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("docagent prove examples/letter.md");
        p.code_block = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = document_xml(&doc);
        assert!(xml.contains(r#"w:val="CodeBlock""#), "{xml}");
        assert!(xml.contains(r#"w:fill="CCFBF1""#), "{xml}");
        let code_p = para_containing(&xml, "docagent prove");
        let pad = hu_to_twips(CODE_PAD_X_HU);
        assert!(
            pad >= 240
                && ind_attr(&code_p, "w:left") == Some(pad)
                && ind_attr(&code_p, "w:right") == Some(pad),
            "code_block must keep letter pre 1rem pad, not jammed on the wash: {code_p}"
        );
        let y_sz = hu_to_border_sz(CODE_PAD_Y_HU);
        assert!(
            code_p.contains("<w:pBdr>")
                && p_bdr_top_sz(&code_p) == Some(y_sz)
                && p_bdr_bottom_sz(&code_p) == Some(y_sz),
            "code_block must keep letter pre 0.75rem Y pad: {code_p}"
        );
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.code_block);
        assert!(p.plain_text().contains("docagent prove"));
    }

    #[test]
    fn write_code_block_pad_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        let mut code = Paragraph::from_text("docagent prove examples/letter.md");
        code.code_block = true;
        section.body.push(Block::Paragraph(code));
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        section
            .body
            .push(Block::Paragraph(task_item("Receiving dock is clear", true)));
        doc.sections.push(section);

        let xml = document_xml(&doc);
        let code_p = para_containing(&xml, "docagent prove");
        let pad = hu_to_twips(CODE_PAD_X_HU);
        assert!(
            pad >= 240 && CODE_PAD_X_HU == 1200,
            "letter pre 1rem is 240 twips, got {pad}"
        );
        assert!(
            code_p.contains(r#"w:val="CodeBlock""#)
                && code_p.contains(&format!(r#"w:fill="{CODE_FILL}""#)),
            "code_block must keep the teal Consolas wash: {code_p}"
        );
        assert_eq!(
            (ind_attr(&code_p, "w:left"), ind_attr(&code_p, "w:right")),
            (Some(pad), Some(pad)),
            "letter pre pad must be 1rem left/right, not jammed on the wash: {code_p}"
        );
        assert!(
            !code_p.contains(r#"w:left="0""#) && !code_p.contains(r#"w:right="0""#),
            "zero CodeBlock indent is WeasyPrint letter pre with no pad: {code_p}"
        );
        let y_sz = hu_to_border_sz(CODE_PAD_Y_HU);
        assert!(
            y_sz >= CODE_PAD_Y_SZ && CODE_PAD_Y_HU == 900,
            "letter pre 0.75rem is OOXML sz 72, got {y_sz}"
        );
        assert!(
            code_p.contains("<w:pBdr>")
                && p_bdr_top_sz(&code_p) == Some(y_sz)
                && p_bdr_bottom_sz(&code_p) == Some(y_sz)
                && code_p.contains(&format!(r#"w:color="{CODE_FILL}""#)),
            "letter pre must keep 0.75rem same-fill Y pad, not a flush wash: {code_p}"
        );
        let code_style = style_fragment(STYLES_XML, "CodeBlock");
        assert_eq!(
            (ind_attr(&code_style, "w:left"), ind_attr(&code_style, "w:right")),
            (Some(pad), Some(pad)),
            "CodeBlock style must keep letter pre 1rem pad: {code_style}"
        );
        assert!(
            code_style.contains("<w:pBdr>")
                && p_bdr_top_sz(&code_style) == Some(y_sz)
                && p_bdr_bottom_sz(&code_style) == Some(y_sz)
                && code_style.contains(&format!(r#"w:color="{CODE_FILL}""#)),
            "CodeBlock style must keep letter pre 0.75rem Y pad: {code_style}"
        );

        let quote_p = para_containing(&xml, "Replay is evidence");
        assert!(
            quote_p.contains("<w:pBdr>")
                && p_bdr_left_sz(&quote_p).unwrap_or(0) >= 24
                && quote_p.contains(&format!(r#"w:color="{QUOTE_COLOR}""#)),
            "code pad must keep quote 4px #134e4a left bar: {quote_p}"
        );
        let hop_tc = cell_containing(&xml, "Hop");
        assert!(
            hop_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && hop_tc.contains(&format!(r#"w:color="{TH_RULE_COLOR}""#)),
            "code pad must keep th teal fill: {hop_tc}"
        );
        let dock_p = para_containing(&xml, "Receiving dock is clear");
        assert_eq!(
            num_id_attr(&dock_p),
            Some(TASK_CHECKED_NUM_ID),
            "code pad must keep checked task numId 4: {dock_p}"
        );
    }

    #[test]
    fn write_code_block_pad_y_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        let mut code = Paragraph::from_text("docagent prove examples/letter.md");
        code.code_block = true;
        section.body.push(Block::Paragraph(code));
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        section
            .body
            .push(Block::Paragraph(task_item("Receiving dock is clear", true)));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![image_run(mark_png(), 2400, 2400, "Harbor mark")];
        section.body.push(Block::Paragraph(img_p));
        doc.sections.push(section);

        let y_sz = hu_to_border_sz(CODE_PAD_Y_HU);
        assert!(
            y_sz >= 72 && CODE_PAD_Y_HU == 900 && CODE_PAD_Y_SZ == 72,
            "letter pre 0.75rem is OOXML sz 72 / 900 HU, got {y_sz}"
        );
        assert_eq!(
            y_sz, CODE_PAD_Y_SZ,
            "CodeBlock Y pad sz must stay in lockstep with CODE_PAD_Y_HU"
        );

        let xml = document_xml(&doc);
        let code_p = para_containing(&xml, "docagent prove");
        assert!(
            code_p.contains(r#"w:val="CodeBlock""#)
                && code_p.contains(&format!(r#"w:fill="{CODE_FILL}""#)),
            "code_block must keep the teal Consolas wash: {code_p}"
        );
        assert_eq!(
            (ind_attr(&code_p, "w:left"), ind_attr(&code_p, "w:right")),
            (Some(hu_to_twips(CODE_PAD_X_HU)), Some(hu_to_twips(CODE_PAD_X_HU))),
            "code Y pad must keep letter pre 1rem X pad: {code_p}"
        );
        assert!(
            code_p.contains("<w:pBdr>")
                && code_p.contains("<w:top ")
                && code_p.contains("<w:bottom "),
            "Word paints pre Y pad as paragraph top/bottom borders: {code_p}"
        );
        assert_eq!(
            (p_bdr_top_sz(&code_p), p_bdr_bottom_sz(&code_p)),
            (Some(y_sz), Some(y_sz)),
            "letter pre Y pad must be 0.75rem (sz 72), not a flush wash: {code_p}"
        );
        assert!(
            code_p.contains(&format!(r#"w:color="{CODE_FILL}""#))
                && !code_p.contains(r#"w:sz="6""#)
                && !code_p.contains(r#"w:sz="8""#),
            "code Y pad must stay the teal wash, not a hairline box: {code_p}"
        );
        assert!(
            !code_p.contains(r#"<w:sz w:val="20"/>"#),
            "code Y pad must not restyle pre as the inline .92em chip: {code_p}"
        );

        let code_style = style_fragment(STYLES_XML, "CodeBlock");
        assert_eq!(
            (p_bdr_top_sz(&code_style), p_bdr_bottom_sz(&code_style)),
            (Some(y_sz), Some(y_sz)),
            "CodeBlock style must keep letter pre 0.75rem Y pad: {code_style}"
        );
        assert!(
            code_style.contains(&format!(r#"w:color="{CODE_FILL}""#))
                && code_style.contains(r#"w:sz="72""#),
            "CodeBlock style Y pad must stay 9pt #ccfbf1: {code_style}"
        );

        let quote_p = para_containing(&xml, "Replay is evidence");
        assert!(
            quote_p.contains("<w:pBdr>")
                && p_bdr_left_sz(&quote_p).unwrap_or(0) >= 24
                && quote_p.contains(&format!(r#"w:color="{QUOTE_COLOR}""#)),
            "code Y pad must keep quote 4px #134e4a left bar: {quote_p}"
        );
        let hop_tc = cell_containing(&xml, "Hop");
        assert!(
            hop_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && hop_tc.contains(&format!(r#"w:color="{TH_RULE_COLOR}""#)),
            "code Y pad must keep th teal fill: {hop_tc}"
        );
        let dock_p = para_containing(&xml, "Receiving dock is clear");
        assert_eq!(
            num_id_attr(&dock_p),
            Some(TASK_CHECKED_NUM_ID),
            "code Y pad must keep checked task numId 4: {dock_p}"
        );
        assert!(
            xml.contains("<a:blip r:embed="),
            "code Y pad must keep 24pt a:blip: {xml}"
        );
        let emu = hu_to_emu(MIN_IMAGE_EDGE_HU);
        assert!(
            xml.contains(&format!(r#"<wp:extent cx="{emu}" cy="{emu}"/>"#)),
            "code Y pad must keep the 24pt a:blip floor: {xml}"
        );

        let bytes = write(&doc).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut styles = String::new();
        zip.by_name("word/styles.xml")
            .unwrap()
            .read_to_string(&mut styles)
            .unwrap();
        let packed = style_fragment(&styles, "CodeBlock");
        assert_eq!(
            (p_bdr_top_sz(&packed), p_bdr_bottom_sz(&packed)),
            (Some(y_sz), Some(y_sz)),
            "packaged CodeBlock must keep letter pre 0.75rem Y pad: {packed}"
        );
    }

    #[test]
    fn roundtrip_keeps_thematic_break() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("before")));
        section.body.push(Block::Break(BreakKind::Thematic));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("after")));
        doc.sections.push(section);
        let xml = document_xml(&doc);
        assert!(xml.contains(r#"w:val="HorizontalLine""#), "{xml}");
        assert!(xml.contains("w:pBdr"), "{xml}");
        let sz = hu_to_border_sz(HR_BORDER_HU);
        let before = hu_to_twips(HR_BEFORE_HU);
        let after = hu_to_twips(HR_AFTER_HU);
        assert!(sz >= 18, "letter 3px is OOXML sz 18, got {sz}");
        assert!(before >= 264 && after >= 264, "letter 1.1rem is 264 twips");
        let hr_p = para_containing(&xml, r#"w:val="HorizontalLine""#);
        assert!(
            hr_p.contains(&format!(r#"w:sz="{sz}""#))
                && hr_p.contains(r#"w:color="134E4A""#),
            "Word paints --- as a 3px teal pBdr, not a missing hairline: {hr_p}"
        );
        assert!(
            spacing_attr(&hr_p, "w:before").unwrap_or(0) >= before
                && spacing_attr(&hr_p, "w:after").unwrap_or(0) >= after,
            "hr must keep letter 1.1rem, not jammed 0pt: {hr_p}"
        );
        let hr_style = style_fragment(STYLES_XML, "HorizontalLine");
        assert!(
            hr_style.contains(r#"w:sz="18""#)
                && spacing_attr(&hr_style, "w:before").unwrap_or(0) >= 264,
            "HorizontalLine style must stay 3px / 1.1rem: {hr_style}"
        );
        let back = roundtrip(&doc).unwrap();
        assert!(
            back.sections[0]
                .body
                .iter()
                .any(|b| matches!(b, Block::Break(BreakKind::Thematic))),
            "{:?}",
            back.sections[0].body
        );
        assert!(back.plain_text().contains("before"));
        assert!(back.plain_text().contains("after"));
    }

    #[test]
    fn roundtrip_keeps_code_span() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Run ");
        let mut code = Run::text("prove");
        code.style.code = true;
        p.runs.push(code);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = document_xml(&doc);
        assert!(xml.contains(r#"w:val="Code""#), "{xml}");
        assert!(xml.contains(&format!(r#"w:fill="{CODE_FILL}""#)), "{xml}");
        let code_p = para_containing(&xml, "prove");
        assert!(
            code_p.contains("Consolas") && code_p.contains(&format!(r#"w:val="{CODE_COLOR}""#)),
            "inline code must be letter teal Consolas, not a bare mono span: {code_p}"
        );
        let prove_run = run_containing(&code_p, ">prove</w:t>");
        assert_eq!(
            style_sz(&prove_run),
            CODE_SPAN_SZ,
            "inline `prove` must stay Word 10pt (.92em of 11pt) even if Code is ignored: {prove_run}"
        );
        assert!(
            prove_run.contains(&format!(r#"<w:szCs w:val="{CODE_SPAN_SZ}"/>"#))
                && !prove_run.contains(r#"<w:sz w:val="22"/>"#)
                && !prove_run.contains(r#"<w:sz w:val="16"/>"#),
            "inline code must not inherit body 11pt or a tofu-scale sz: {prove_run}"
        );
        assert!(
            bdr_space_for_color(&code_p, CODE_FILL).unwrap_or(0) >= CODE_BDR_SPACE_PT,
            "inline code must keep letter .1em/.35em Word rPr padding: {code_p}"
        );
        let code_style = style_fragment(STYLES_XML, "Code");
        assert!(
            code_style.contains("Consolas")
                && code_style.contains(&format!(r#"w:val="{CODE_COLOR}""#))
                && code_style.contains(&format!(r#"w:fill="{CODE_FILL}""#)),
            "Code style must stay letter teal Consolas chip: {code_style}"
        );
        assert!(
            bdr_space_for_color(&code_style, CODE_FILL).unwrap_or(0) >= CODE_BDR_SPACE_PT,
            "Code style must keep letter pad: {code_style}"
        );
        assert_eq!(style_sz(&code_style), CODE_SPAN_SZ, "{code_style}");
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.code && matches!(&r.content, RunContent::Text(t) if t == "prove")
        }));
        assert!(p.runs.iter().any(|r| {
            !r.style.code && matches!(&r.content, RunContent::Text(t) if t.contains("Run"))
        }));
    }

    #[test]
    fn roundtrip_keeps_strikethrough() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("plain ");
        let mut strike = Run::text("guess");
        strike.style.strike = true;
        p.runs.push(strike);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = document_xml(&doc);
        let guess_p = para_containing(&xml, "guess");
        assert!(
            guess_p.contains(r#"w:val="Strike""#)
                && guess_p.contains("<w:strike/>")
                && guess_p.contains(">guess</w:t>"),
            "~~guess~~ must emit Strike + w:strike so Word paints the line, not missing: {guess_p}"
        );
        let strike_style = style_fragment(STYLES_XML, "Strike");
        assert!(
            strike_style.contains("<w:strike/>")
                && !strike_style.contains(r#"w:val="none""#)
                && !strike_style.contains(r#"w:val="0""#),
            "Strike style must be Word strikethrough, not a missing line: {strike_style}"
        );
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.strike && matches!(&r.content, RunContent::Text(t) if t.contains("guess"))
        }));
        assert!(p.runs.iter().any(|r| {
            !r.style.strike && matches!(&r.content, RunContent::Text(t) if t.contains("plain"))
        }));
    }

    #[test]
    fn roundtrip_keeps_list_numbering() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut a = Paragraph::from_text("Receiving dock is clear");
        a.numbering = Some(NumberingRef {
            definition_id: 0,
            level: 0,
            start: None,
            format: Some(NumberFormat::Bullet),
        });
        let mut b = Paragraph::from_text("Bay 4, not bay 2");
        b.numbering = Some(NumberingRef {
            definition_id: 0,
            level: 1,
            start: None,
            format: Some(NumberFormat::Bullet),
        });
        let mut c = Paragraph::from_text("Hash the input");
        c.numbering = Some(NumberingRef {
            definition_id: 1,
            level: 0,
            start: Some(1),
            format: Some(NumberFormat::Decimal),
        });
        let mut d = Paragraph::from_text("Run Convert");
        d.numbering = Some(NumberingRef {
            definition_id: 1,
            level: 0,
            start: Some(2),
            format: Some(NumberFormat::Decimal),
        });
        section.body.push(Block::Paragraph(a));
        section.body.push(Block::Paragraph(b));
        section.body.push(Block::Paragraph(c));
        section.body.push(Block::Paragraph(d));
        doc.sections.push(section);
        let xml = document_xml(&doc);
        assert!(xml.contains("<w:numPr>"), "{xml}");
        assert!(xml.contains(r#"w:val="1""#), "{xml}");
        assert!(xml.contains(r#"w:val="2""#), "{xml}");
        let bytes = write(&doc).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        assert!(zip.by_name("word/numbering.xml").is_ok());
        let back = roundtrip(&doc).unwrap();
        let nums: Vec<_> = back.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => p.numbering.as_ref().map(|n| (n.format, n.level, n.start)),
                _ => None,
            })
            .collect();
        assert_eq!(
            nums,
            [
                (Some(NumberFormat::Bullet), 0, None),
                (Some(NumberFormat::Bullet), 1, None),
                (Some(NumberFormat::Decimal), 0, Some(1)),
                (Some(NumberFormat::Decimal), 0, Some(2)),
            ]
        );
    }

    fn numbered(text: &str, level: u8, format: NumberFormat) -> Paragraph {
        let mut p = Paragraph::from_text(text);
        p.numbering = Some(NumberingRef {
            definition_id: u32::from(level),
            level,
            start: None,
            format: Some(format),
        });
        p
    }

    #[test]
    fn roundtrip_keeps_nested_list_ilvl() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Paragraph(numbered(
            "dock",
            0,
            NumberFormat::Bullet,
        )));
        section.body.push(Block::Paragraph(numbered(
            "bay 4",
            1,
            NumberFormat::Bullet,
        )));
        section.body.push(Block::Paragraph(numbered(
            "pallet",
            2,
            NumberFormat::Bullet,
        )));
        section.body.push(Block::Paragraph(numbered(
            "hash",
            0,
            NumberFormat::Decimal,
        )));
        section.body.push(Block::Paragraph(numbered(
            "convert",
            1,
            NumberFormat::Decimal,
        )));
        doc.sections.push(section);
        let xml = document_xml(&doc);
        assert!(xml.contains(r#"<w:ilvl w:val="0"/>"#), "{xml}");
        assert!(xml.contains(r#"<w:ilvl w:val="1"/>"#), "{xml}");
        assert!(xml.contains(r#"<w:ilvl w:val="2"/>"#), "{xml}");
        let back = roundtrip(&doc).unwrap();
        let levels: Vec<_> = back.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => p.numbering.as_ref().map(|n| (p.plain_text(), n.level)),
                _ => None,
            })
            .collect();
        assert_eq!(
            levels,
            [
                ("dock".into(), 0),
                ("bay 4".into(), 1),
                ("pallet".into(), 2),
                ("hash".into(), 0),
                ("convert".into(), 1),
            ]
        );
    }

    #[test]
    fn reads_python_docx_nested_list_ilvl() {
        let document_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="ListParagraph"/><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>dock</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="ListParagraph"/><w:numPr><w:ilvl w:val="1"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>bay 4</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="ListParagraph"/><w:numPr><w:ilvl w:val="2"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>pallet</w:t></w:r></w:p></w:body></w:document>"#;
        let pkg = pack_docx(&[
            ("[Content_Types].xml", MINIMAL_CONTENT_TYPES),
            ("_rels/.rels", MINIMAL_RELS),
            ("word/document.xml", document_xml),
        ]);
        let back = read(&pkg).unwrap();
        let levels: Vec<_> = back.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => p.numbering.as_ref().map(|n| (p.plain_text(), n.level)),
                _ => None,
            })
            .collect();
        assert_eq!(
            levels,
            [
                ("dock".into(), 0),
                ("bay 4".into(), 1),
                ("pallet".into(), 2),
            ]
        );
    }

    #[test]
    fn reads_python_docx_list_number_2_style_ilvl() {
        let styles_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="ListParagraph"><w:name w:val="List Paragraph"/></w:style><w:style w:type="paragraph" w:styleId="ListNumber2"><w:name w:val="List Number 2"/><w:basedOn w:val="ListParagraph"/><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="2"/></w:numPr></w:pPr></w:style></w:styles>"#;
        let document_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="ListNumber2"/></w:pPr><w:r><w:t>nested via style</w:t></w:r></w:p></w:body></w:document>"#;
        let pkg = pack_docx(&[
            ("[Content_Types].xml", MINIMAL_CONTENT_TYPES),
            ("_rels/.rels", MINIMAL_RELS),
            ("word/styles.xml", styles_xml),
            ("word/document.xml", document_xml),
        ]);
        let back = read(&pkg).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph {:?}", back.sections[0].body);
        };
        assert_eq!(p.plain_text(), "nested via style");
        let n = p.numbering.as_ref().expect("numbering from ListNumber2");
        assert_eq!(n.level, 1);
        assert_eq!(n.format, Some(NumberFormat::Decimal));
    }

    #[test]
    fn roundtrip_keeps_hyperlink() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![Run::hyperlink(
            "DocAgent",
            "https://github.com/kevin9327/docagent",
        )];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = document_xml(&doc);
        assert!(xml.contains("w:hyperlink"), "{xml}");
        assert!(xml.contains(r#"r:id="rId3""#), "{xml}");
        assert!(xml.contains(r#"w:val="Hyperlink""#), "{xml}");
        assert!(xml.contains(r#"<w:u w:val="single"/>"#), "{xml}");
        assert!(xml.contains(r#"w:val="0D9488""#), "{xml}");
        assert!(xml.contains(">DocAgent</w:t>"), "{xml}");
        let link_style = style_fragment(STYLES_XML, "Hyperlink");
        assert!(
            link_style.contains(r#"w:val="0D9488""#) && link_style.contains("<w:u "),
            "Hyperlink style must stay letter teal + underline: {link_style}"
        );
        let rels = document_rels(&doc);
        assert!(rels.contains("hyperlink"), "{rels}");
        assert!(rels.contains("https://github.com/kevin9327/docagent"), "{rels}");
        assert!(rels.contains(r#"TargetMode="External""#), "{rels}");
        assert!(rels.contains(r#"Id="rId3""#), "{rels}");
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("para");
        };
        assert!(p.runs.iter().any(|r| matches!(
            &r.content,
            RunContent::Inline(InlineObject::Hyperlink { target, display })
                if target == "https://github.com/kevin9327/docagent" && display == "DocAgent"
        )));
        assert!(p.runs.iter().any(|r| {
            r.style.underline == Underline::Single
                && matches!(&r.content, RunContent::Inline(InlineObject::Hyperlink { .. }))
        }));
    }

    #[test]
    fn write_hyperlink_rid_follows_images() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![image_run(mark_png(), 2400, 2400, "Harbor mark")];
        section.body.push(Block::Paragraph(img_p));
        let mut link_p = Paragraph::from_text("");
        link_p.runs = vec![
            Run::hyperlink("DocAgent", "https://github.com/kevin9327/docagent"),
            Run::text(" runtime"),
        ];
        section.body.push(Block::Paragraph(link_p));
        doc.sections.push(section);
        let xml = document_xml(&doc);
        assert!(xml.contains(r#"<a:blip r:embed="rId3"/>"#), "{xml}");
        assert!(xml.contains(r#"<w:hyperlink r:id="rId4">"#), "{xml}");
        assert!(
            xml.contains(r#"<w:rStyle w:val="Hyperlink"/>"#)
                && xml.contains(r#"<w:u w:val="single"/>"#)
                && xml.contains(r#"w:val="0D9488""#),
            "link must stay visible next to a:blip: {xml}"
        );
        let rels = document_rels(&doc);
        assert!(rels.contains(r#"Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image""#), "{rels}");
        assert!(
            rels.contains(r#"Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://github.com/kevin9327/docagent" TargetMode="External""#),
            "{rels}"
        );
        let back = roundtrip(&doc).unwrap();
        assert!(back.sections[0].body.iter().any(|b| match b {
            Block::Paragraph(p) => p.runs.iter().any(|r| matches!(
                &r.content,
                RunContent::Inline(InlineObject::Hyperlink { display, target })
                    if display == "DocAgent" && target == "https://github.com/kevin9327/docagent"
            )),
            _ => false,
        }));
        assert!(back.sections[0].body.iter().any(|b| match b {
            Block::Paragraph(p) => p
                .runs
                .iter()
                .any(|r| matches!(&r.content, RunContent::Inline(InlineObject::Image(_)))),
            _ => false,
        }));
    }

    #[test]
    fn roundtrip_keeps_heading_outline() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.bold = true;
        h1.runs[0].style.size = 1800;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation");
        h2.outline_level = Some(2);
        h2.runs[0].style.bold = true;
        h2.runs[0].style.size = 1400;
        section.body.push(Block::Paragraph(h2));
        doc.sections.push(section);
        let xml = document_xml(&doc);
        assert!(xml.contains(r#"w:val="Heading1""#), "{xml}");
        assert!(xml.contains(r#"w:val="Heading2""#), "{xml}");
        assert!(xml.contains(r#"w:outlineLvl w:val="0""#), "{xml}");
        assert!(
            !xml.contains(r#"w:sz w:val="36""#),
            "Heading1 must not keep markdown 18pt over Word 16pt: {xml}"
        );
        assert!(
            !xml.contains(r#"w:sz w:val="28""#),
            "Heading2 must not keep markdown 14pt over Word 13pt: {xml}"
        );
        let bytes = write(&doc).unwrap();
        assert!(sniff(&bytes));
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("h1");
        };
        assert_eq!(p.outline_level, Some(1));
        assert!(p.runs[0].style.bold);
        assert_eq!(p.runs[0].style.size, HEADING1_SIZE_HU);
        let Block::Paragraph(p2) = &back.sections[0].body[1] else {
            panic!("h2");
        };
        assert_eq!(p2.outline_level, Some(2));
        assert_eq!(p2.runs[0].style.size, HEADING2_SIZE_HU);
    }

    #[test]
    fn roundtrip_keeps_cell_emphasis() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![vec!["plain".into()]]);
        let mut p = Paragraph::from_text("");
        let mut bold = Run::text("GPU-free");
        bold.style.bold = true;
        p.runs = vec![bold];
        let mut italic = Run::text(" byte-for-byte");
        italic.style.italic = true;
        p.runs.push(italic);
        table.rows[0].cells[0].blocks = vec![Block::Paragraph(p)];
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let xml = document_xml(&doc);
        assert!(xml.contains("<w:b/>"), "{xml}");
        assert!(xml.contains("<w:i/>"), "{xml}");
        let back = roundtrip(&doc).unwrap();
        let Block::Table(t) = &back.sections[0].body[0] else {
            panic!("table");
        };
        let Block::Paragraph(cell) = &t.rows[0].cells[0].blocks[0] else {
            panic!("cell para");
        };
        assert!(cell.runs.iter().any(|r| {
            r.style.bold && matches!(&r.content, RunContent::Text(t) if t.contains("GPU-free"))
        }));
        assert!(cell.runs.iter().any(|r| {
            r.style.italic && matches!(&r.content, RunContent::Text(t) if t.contains("byte-for-byte"))
        }));
    }

    #[test]
    fn roundtrip_keeps_table_header() {
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
        let xml = document_xml(&doc);
        assert!(xml.contains("<w:tblHeader/>"), "{xml}");
        let hop_tc = cell_containing(&xml, "Hop");
        let input_tc = cell_containing(&xml, "Input");
        let rule_sz = hu_to_border_sz(TH_RULE_HU);
        assert!(
            hop_tc.contains(&format!(r#"w:fill="{TH_FILL}""#)),
            "header Hop must keep letter #ccfbf1 fill, not a sparse grid: {hop_tc}"
        );
        assert!(
            hop_tc.contains("<w:tcBorders>")
                && hop_tc.contains(&format!(r#"w:sz="{rule_sz}""#))
                && hop_tc.contains(&format!(r#"w:color="{TH_RULE_COLOR}""#)),
            "header Hop must keep letter 2px #0d9488 rule: {hop_tc}"
        );
        assert!(
            !input_tc.contains(&format!(r#"w:fill="{TH_FILL}""#)),
            "body Input must not keep th fill: {input_tc}"
        );
        let hop_p = para_containing(&xml, "Hop");
        assert!(
            hop_p.contains("<w:b/>") && hop_p.contains(&format!(r#"w:val="{TH_COLOR}""#)),
            "header Hop text must stay letter teal bold: {hop_p}"
        );
        assert!(
            hop_tc.contains(&format!(r#"<w:vAlign w:val="{CELL_VALIGN}"/>"#))
                && input_tc.contains(&format!(r#"<w:vAlign w:val="{CELL_VALIGN}"/>"#)),
            "header/body cells must stay vertical-align top: {hop_tc} {input_tc}"
        );
        let table_grid = style_fragment(STYLES_XML, "TableGrid");
        assert!(
            table_grid.contains(r#"w:type="firstRow""#)
                && table_grid.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && table_grid.contains(&format!(r#"w:color="{TH_RULE_COLOR}""#)),
            "TableGrid firstRow must keep letter th fill + teal rule: {table_grid}"
        );
        let back = roundtrip(&doc).unwrap();
        let Block::Table(t) = &back.sections[0].body[0] else {
            panic!("table");
        };
        assert!(t.rows[0].header);
        assert!(!t.rows[1].header);
        assert_eq!(t.header_row_count, 1);
    }

    #[test]
    fn write_table_type_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        section.body.push(Block::Paragraph(h1));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        let mut prove_p = Paragraph::from_text("");
        let mut prove = Run::text("prove");
        prove.style.code = true;
        prove_p.runs.push(prove);
        table.rows[1].cells[1].blocks = vec![Block::Paragraph(prove_p)];
        section.body.push(Block::Table(table));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        section
            .body
            .push(Block::Paragraph(task_item("Receiving dock is clear", true)));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![image_run(mark_png(), 2400, 2400, "Harbor mark")];
        section.body.push(Block::Paragraph(img_p));
        doc.sections.push(section);

        let xml = document_xml(&doc);
        assert_eq!(CELL_SZ, 23, "letter 0.95rem is OOXML sz 23 (11.5pt)");
        let hop_run = run_containing(&xml, ">Hop</w:t>");
        let input_run = run_containing(&xml, ">Input</w:t>");
        assert_eq!(
            style_sz(&hop_run),
            CELL_SZ,
            "letter th must stay 0.95rem (sz 23), not body 11pt: {hop_run}"
        );
        assert_eq!(
            style_sz(&input_run),
            CELL_SZ,
            "letter td must stay 0.95rem (sz 23), not body 11pt: {input_run}"
        );
        assert!(
            hop_run.contains(&format!(r#"<w:szCs w:val="{CELL_SZ}"/>"#))
                && !hop_run.contains(r#"<w:sz w:val="22"/>"#)
                && !hop_run.contains(r#"<w:sz w:val="20"/>"#)
                && !input_run.contains(r#"<w:sz w:val="22"/>"#)
                && !input_run.contains(r#"<w:sz w:val="20"/>"#),
            "hop table type must not inherit body 11pt or the code chip 10pt: {hop_run} {input_run}"
        );
        let prove_run = run_containing(&xml, ">prove</w:t>");
        assert_eq!(
            style_sz(&prove_run),
            CODE_SPAN_SZ,
            "inline `prove` in a cell must stay Word 10pt, not table 0.95rem: {prove_run}"
        );
        let body_run = run_containing(&xml, ">6 September 2026</w:t>");
        assert!(
            !body_run.contains(&format!(r#"<w:sz w:val="{CELL_SZ}"/>"#))
                && !body_run.contains(r#"<w:sz w:val="20"/>"#),
            "body type must stay Word 11pt, not hop-table 0.95rem: {body_run}"
        );
        let h1_p = para_containing(&xml, r#"w:val="Heading1""#);
        assert!(
            !h1_p.contains("<w:sz "),
            "table type must not reintroduce markdown CSS w:sz on headings: {h1_p}"
        );
        let table_grid = style_fragment(STYLES_XML, "TableGrid");
        assert!(
            table_grid.contains(&format!(r#"<w:sz w:val="{CELL_SZ}"/>"#))
                && table_grid.contains(&format!(r#"<w:szCs w:val="{CELL_SZ}"/>"#)),
            "TableGrid must keep letter 0.95rem so style-only cells are not body 11pt: {table_grid}"
        );
        assert!(
            table_grid.contains(&format!(r#"<w:vAlign w:val="{CELL_VALIGN}"/>"#)),
            "TableGrid must keep letter vertical-align top: {table_grid}"
        );
        let hop_tc = cell_containing(&xml, "Hop");
        assert!(
            hop_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && hop_tc.contains(&format!(r#"w:color="{TH_RULE_COLOR}""#))
                && hop_tc.contains("<w:tcBorders>"),
            "table type must keep th teal fill: {hop_tc}"
        );
        let quote_p = para_containing(&xml, "Replay is evidence");
        assert!(
            quote_p.contains("<w:pBdr>")
                && quote_p.contains("<w:left ")
                && p_bdr_left_sz(&quote_p).unwrap_or(0) >= 24,
            "table type must keep quote 4px left bar: {quote_p}"
        );
        let dock_p = para_containing(&xml, "Receiving dock is clear");
        assert_eq!(
            num_id_attr(&dock_p),
            Some(TASK_CHECKED_NUM_ID),
            "table type must keep checked task numId 4: {dock_p}"
        );
        let min_emu = hu_to_emu(MIN_IMAGE_EDGE_HU);
        assert!(
            xml.contains("<a:blip r:embed=")
                && xml.contains(&format!(r#"<wp:extent cx="{min_emu}" cy="{min_emu}"/>"#)),
            "table type must keep 24pt a:blip: {xml}"
        );
    }

    #[test]
    fn write_table_cell_valign_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        section.body.push(Block::Paragraph(h1));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into(), "Proof".into()],
            vec![
                "Input".into(),
                "layout i32 @ 1/7200 in".into(),
                "input SHA-256".into(),
            ],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        section
            .body
            .push(Block::Paragraph(task_item("Receiving dock is clear", true)));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![image_run(mark_png(), 2400, 2400, "Harbor mark")];
        section.body.push(Block::Paragraph(img_p));
        doc.sections.push(section);

        let xml = document_xml(&doc);
        assert_eq!(CELL_VALIGN, "top", "letter th,td vertical-align is OOXML top");
        let hop_tc = cell_containing(&xml, "Hop");
        let file_tc = cell_containing(&xml, "File");
        let proof_tc = cell_containing(&xml, "Proof");
        let input_tc = cell_containing(&xml, "Input");
        let plan_tc = cell_containing(&xml, "layout i32");
        let sha_tc = cell_containing(&xml, "input SHA-256");
        for (name, tc) in [
            ("Hop", &hop_tc),
            ("File", &file_tc),
            ("Proof", &proof_tc),
            ("Input", &input_tc),
            ("plan", &plan_tc),
            ("SHA", &sha_tc),
        ] {
            assert!(
                tc.contains(&format!(r#"<w:vAlign w:val="{CELL_VALIGN}"/>"#)),
                "letter {name} must keep th,td vertical-align top: {tc}"
            );
            assert!(
                !tc.contains(r#"w:val="center""#)
                    && !tc.contains(r#"w:val="both""#)
                    && !tc.contains(r#"w:val="bottom""#),
                "UA/python-docx center {name} punches sparse bands vs WeasyPrint: {tc}"
            );
        }
        assert!(
            hop_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && hop_tc.contains(&format!(r#"w:color="{TH_RULE_COLOR}""#))
                && hop_tc.contains("<w:tcBorders>"),
            "cell valign must keep th teal fill: {hop_tc}"
        );
        assert!(
            !input_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && !sha_tc.contains(&format!(r#"w:fill="{TH_FILL}""#)),
            "cell valign must not leak th fill onto td: {input_tc} {sha_tc}"
        );
        let hop_run = run_containing(&xml, ">Hop</w:t>");
        let input_run = run_containing(&xml, ">Input</w:t>");
        assert_eq!(
            style_sz(&hop_run),
            CELL_SZ,
            "cell valign must keep letter 0.95rem table type: {hop_run}"
        );
        assert_eq!(
            style_sz(&input_run),
            CELL_SZ,
            "cell valign must keep letter td 0.95rem: {input_run}"
        );
        let table_grid = style_fragment(STYLES_XML, "TableGrid");
        assert!(
            table_grid.contains(&format!(r#"<w:vAlign w:val="{CELL_VALIGN}"/>"#))
                && !table_grid.contains(r#"w:val="center""#),
            "TableGrid must keep letter vertical-align top so style-only cells are not middle: {table_grid}"
        );
        assert!(
            table_grid.contains(r#"w:type="firstRow""#)
                && table_grid.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && table_grid.contains(&format!(r#"<w:sz w:val="{CELL_SZ}"/>"#)),
            "cell valign must keep TableGrid firstRow fill + 0.95rem: {table_grid}"
        );
        let body_p = para_containing(&xml, "6 September 2026");
        assert!(
            body_p.contains(&format!(r#"<w:color w:val="{BODY_COLOR}"/>"#))
                && !body_p.contains("<w:vAlign "),
            "cell valign must keep body #134e4a and not leak onto Normal: {body_p}"
        );
        let quote_p = para_containing(&xml, "Replay is evidence");
        assert!(
            quote_p.contains("<w:pBdr>")
                && quote_p.contains("<w:left ")
                && p_bdr_left_sz(&quote_p).unwrap_or(0) >= 24,
            "cell valign must keep quote 4px left bar: {quote_p}"
        );
        let dock_p = para_containing(&xml, "Receiving dock is clear");
        assert_eq!(
            num_id_attr(&dock_p),
            Some(TASK_CHECKED_NUM_ID),
            "cell valign must keep checked task numId 4: {dock_p}"
        );
        let h1_p = para_containing(&xml, r#"w:val="Heading1""#);
        assert!(
            !h1_p.contains("<w:sz "),
            "cell valign must not reintroduce markdown CSS w:sz on headings: {h1_p}"
        );
        let min_emu = hu_to_emu(MIN_IMAGE_EDGE_HU);
        assert!(
            xml.contains("<a:blip r:embed=")
                && xml.contains(&format!(r#"<wp:extent cx="{min_emu}" cy="{min_emu}"/>"#)),
            "cell valign must keep 24pt a:blip: {xml}"
        );
    }

    #[test]
    fn roundtrip_text_and_page() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("DOCX hello")));
        doc.sections.push(section);
        let back = roundtrip(&doc).unwrap();
        assert!(back.plain_text().contains("DOCX hello"));
        let delta = (back.sections[0].page.width - doc.sections[0].page.width).abs();
        assert!(
            delta < docagent_model::HU_PER_TWIP,
            "page width quantized beyond one twip: {delta}"
        );
    }

    #[test]
    fn roundtrip_keeps_image_bytes() {
        let png = mark_png();
        assert!(
            png.starts_with(&[0x89, b'P', b'N', b'G']),
            "fixture must be PNG"
        );
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![image_run(png.clone(), 7200, 3600, "DocAgent mark")];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);

        let xml = document_xml(&doc);
        assert!(xml.contains("<w:drawing>"), "{xml}");
        assert!(xml.contains("<a:blip r:embed="), "{xml}");
        assert!(
            xml.contains(r#"<pic:pic xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture">"#),
            "python-docx puts xmlns:pic on pic:pic so Word paints a:blip: {xml}"
        );
        assert!(
            xml.contains(r#"<a:graphic xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">"#),
            "python-docx puts xmlns:a on a:graphic so Word paints a:blip: {xml}"
        );
        assert!(xml.contains(r#"cx="914400""#), "{xml}");
        assert!(xml.contains(r#"cy="457200""#), "{xml}");
        assert!(xml.contains(r#"descr="DocAgent mark""#), "{xml}");

        let rels = document_rels(&doc);
        assert!(rels.contains("/relationships/image"), "{rels}");
        assert!(rels.contains("media/image1.png"), "{rels}");

        let bytes = write(&doc).unwrap();
        assert!(sniff(&bytes));
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.clone())).unwrap();
        let mut media = Vec::new();
        zip.by_name("word/media/image1.png")
            .unwrap()
            .read_to_end(&mut media)
            .unwrap();
        assert_eq!(media, png);

        let types = {
            let mut f = zip.by_name("[Content_Types].xml").unwrap();
            let mut s = String::new();
            f.read_to_string(&mut s).unwrap();
            s
        };
        assert!(types.contains(r#"Extension="png""#), "{types}");
        assert!(types.contains("image/png"), "{types}");

        let back = read(&bytes).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph {:?}", back.sections[0].body);
        };
        let Some(img) = p.runs.iter().find_map(|r| match &r.content {
            RunContent::Inline(InlineObject::Image(img)) => Some(img),
            _ => None,
        }) else {
            panic!("missing image run: {:?}", p.runs);
        };
        assert_eq!(img.bytes, png);
        assert_eq!(img.mime, "image/png");
        assert_eq!(img.width, 7200);
        assert_eq!(img.height, 3600);
        assert_eq!(img.alt_text.as_deref(), Some("DocAgent mark"));
        assert_eq!(img.wrap, WrapMode::Inline);
    }

    #[test]
    fn write_floors_sub_24pt_image_extent() {
        // 16px at 96dpi is 12pt (1200 HU); Word must paint the 24pt floor.
        let png = mark_png();
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![image_run(png, 1200, 1200, "Harbor mark")];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = document_xml(&doc);
        let (cx, cy) = visible_image_size(1200, 1200);
        assert_eq!((cx, cy), (MIN_IMAGE_EDGE_HU, MIN_IMAGE_EDGE_HU));
        let emu = hu_to_emu(MIN_IMAGE_EDGE_HU);
        assert!(emu >= 304_800, "24pt in EMU: {emu}");
        assert!(
            xml.contains(&format!(r#"<wp:extent cx="{emu}" cy="{emu}"/>"#)),
            "sub-24pt image must write a visible wp:extent: {xml}"
        );
        assert!(xml.contains("<a:blip r:embed="), "{xml}");
        assert!(xml.contains(r#"descr="Harbor mark""#), "{xml}");
        let mut zero_doc = Document::new();
        let mut zero_section = Section::default();
        let mut zero_p = Paragraph::from_text("");
        zero_p.runs = vec![image_run(mark_png(), 0, 0, "Harbor mark")];
        zero_section.body.push(Block::Paragraph(zero_p));
        zero_doc.sections.push(zero_section);
        let zero = document_xml(&zero_doc);
        assert!(
            zero.contains(&format!(r#"<wp:extent cx="{emu}" cy="{emu}"/>"#)),
            "0×0 image must not write a 127-EMU speck: {zero}"
        );
    }

    #[test]
    fn write_table_emits_borders_and_page_width() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into(), "Proof".into()],
            vec!["Input".into(), "letter.md".into(), "input SHA-256".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let xml = document_xml(&doc);
        assert!(xml.contains("<w:tbl>"), "{xml}");
        assert!(xml.contains(r#"w:val="TableGrid""#), "{xml}");
        assert!(xml.contains("<w:tblBorders>"), "{xml}");
        let grid = tbl_borders_block(&xml);
        let grid_sz = hu_to_border_sz(TBL_BORDER_HU);
        assert!(
            grid_sz >= 6
                && grid.contains(&format!(r#"w:sz="{grid_sz}""#))
                && grid.contains(&format!(r#"w:color="{TBL_BORDER_COLOR}""#))
                && !grid.contains(r#"w:color="000000""#),
            "hop grid must be letter 1px #cbd5e1, not Word TableGrid black: {grid}"
        );
        assert!(xml.contains("<w:insideH"), "{xml}");
        assert!(xml.contains("<w:insideV"), "{xml}");
        assert!(xml.contains(r#"<w:tblW w:w=""#), "{xml}");
        let content_twips = hu_to_twips(letter_content_width(&doc.sections[0].page));
        assert!(
            xml.contains(&format!(r#"<w:tblW w:w="{content_twips}" w:type="dxa"/>"#)),
            "letter-style table must span the page content width, not stay tiny: {xml}"
        );
        assert!(
            content_twips > hu_to_twips(doc.sections[0].page.content_width()),
            "letter hop table must span @page 2.75rem measure, not Hangul 30mm gutters: {content_twips}"
        );
        assert!(xml.contains("Hop"), "{xml}");
        assert!(xml.contains("letter.md"), "{xml}");
    }

    #[test]
    fn write_page_margin_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        assert_eq!(doc.sections[0].page.margin_left, DEFAULT_MARGIN_HU);
        assert_eq!(doc.sections[0].page.margin_top, DEFAULT_MARGIN_HU);
        let xml = document_xml(&doc);

        let x = hu_to_twips(PAGE_MARGIN_X_HU);
        let y = hu_to_twips(PAGE_MARGIN_Y_HU);
        let hangul = hu_to_twips(DEFAULT_MARGIN_HU);
        let ua75 = hu_to_twips(75 * (HU_PER_INCH / 96));
        assert_eq!(
            (
                pg_mar_attr(&xml, "w:left"),
                pg_mar_attr(&xml, "w:right"),
                pg_mar_attr(&xml, "w:top"),
                pg_mar_attr(&xml, "w:bottom")
            ),
            (Some(x), Some(x), Some(y), Some(y)),
            "letter pgMar must be @page 2.5rem/2.75rem, not Hangul 30mm: {xml}"
        );
        assert!(
            x == 660 && y == 600 && hangul == 1700 && ua75 == 1125,
            "letter 2.75rem/2.5rem is 660/600 twips; Hangul 30mm is 1700; UA 75px is 1125"
        );
        assert!(
            !xml.contains(&format!(r#"w:left="{hangul}""#))
                && !xml.contains(&format!(r#"w:top="{hangul}""#))
                && !xml.contains(r#"w:left="1440""#)
                && !xml.contains(&format!(r#"w:left="{ua75}""#)),
            "Hangul 30mm / Word 1in / WeasyPrint UA 75px is a sparse frame: {xml}"
        );
        assert_eq!(
            (pg_mar_attr(&xml, "w:header"), pg_mar_attr(&xml, "w:footer")),
            (Some(y / 2), Some(y / 2)),
            "header/footer must sit inside letter 2.5rem, not Hangul 15mm: {xml}"
        );

        let content_twips = hu_to_twips(letter_content_width(&doc.sections[0].page));
        assert!(
            xml.contains(&format!(r#"<w:tblW w:w="{content_twips}" w:type="dxa"/>"#))
                && content_twips > hu_to_twips(doc.sections[0].page.content_width()),
            "hop table must span the letter measure, not Hangul 30mm gutters: {xml}"
        );

        let mut custom = Document::new();
        let mut custom_section = Section::default();
        custom_section.page.margin_left = HU_PER_INCH;
        custom_section.page.margin_right = HU_PER_INCH;
        custom_section.page.margin_top = HU_PER_INCH;
        custom_section.page.margin_bottom = HU_PER_INCH;
        custom_section
            .body
            .push(Block::Paragraph(Paragraph::from_text("custom")));
        custom.sections.push(custom_section);
        let custom_xml = document_xml(&custom);
        let inch = hu_to_twips(HU_PER_INCH);
        assert_eq!(
            (
                pg_mar_attr(&custom_xml, "w:left"),
                pg_mar_attr(&custom_xml, "w:top")
            ),
            (Some(inch), Some(inch)),
            "author 1in pgMar must not remap to letter 2.75rem: {custom_xml}"
        );

        let body_p = para_containing(&xml, "6 September 2026");
        let body_run = run_containing(&body_p, ">6 September 2026</w:t>");
        assert!(
            body_run.contains(&format!(r#"<w:color w:val="{BODY_COLOR}"/>"#))
                && body_run.contains(r#"w:ascii="Segoe UI""#),
            "page margin must keep Segoe UI body #134e4a: {body_run}"
        );
        assert_eq!(
            spacing_attr(&body_p, "w:line"),
            Some(BODY_LINE),
            "page margin must keep Word 1.15: {body_p}"
        );
        let quote_p = para_containing(&xml, "Replay is evidence");
        assert!(
            quote_p.contains("<w:pBdr>")
                && quote_p.contains("<w:left ")
                && p_bdr_left_sz(&quote_p).unwrap_or(0) >= 24
                && quote_p.contains(&format!(r#"w:color="{QUOTE_COLOR}""#)),
            "page margin must keep quote 4px #134e4a left bar: {quote_p}"
        );
        let hop_tc = cell_containing(&xml, "Hop");
        assert!(
            hop_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && hop_tc.contains(&format!(r#"w:color="{TH_RULE_COLOR}""#))
                && hop_tc.contains("<w:tcBorders>"),
            "page margin must keep th teal fill: {hop_tc}"
        );
    }

    #[test]
    fn write_table_black_grid_uses_letter_slate() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        section
            .body
            .push(Block::Paragraph(task_item("Receiving dock is clear", true)));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![image_run(mark_png(), 2400, 2400, "Harbor mark")];
        section.body.push(Block::Paragraph(img_p));
        doc.sections.push(section);

        let xml = document_xml(&doc);
        let grid = tbl_borders_block(&xml);
        let grid_sz = hu_to_border_sz(TBL_BORDER_HU);
        assert!(
            grid_sz >= 6
                && grid.contains(&format!(r#"w:sz="{grid_sz}""#))
                && grid.contains(&format!(r#"w:color="{TBL_BORDER_COLOR}""#))
                && grid.contains("<w:insideH")
                && grid.contains("<w:insideV")
                && !grid.contains(r#"w:color="000000""#),
            "letter hop grid must be 1px #cbd5e1, not Word TableGrid black: {grid}"
        );
        let table_grid = style_fragment(STYLES_XML, "TableGrid");
        assert!(
            table_grid.contains(&format!(r#"w:color="{TBL_BORDER_COLOR}""#))
                && table_grid.contains(r#"w:sz="6""#)
                && !table_grid.contains(r#"w:color="000000""#),
            "TableGrid must keep letter 1px #cbd5e1: {table_grid}"
        );

        let body_p = para_containing(&xml, "6 September 2026");
        assert!(
            body_p.contains(&format!(r#"<w:color w:val="{BODY_COLOR}"/>"#))
                && spacing_attr(&body_p, "w:line").unwrap_or(0) >= BODY_LINE,
            "slate grid must keep body #134e4a + line 276: {body_p}"
        );
        let quote_p = para_containing(&xml, "Replay is evidence");
        assert!(
            quote_p.contains("<w:pBdr>")
                && p_bdr_left_sz(&quote_p).unwrap_or(0) >= 24
                && quote_p.contains(&format!(r#"w:color="{QUOTE_COLOR}""#)),
            "slate grid must keep quote 4px bar: {quote_p}"
        );
        let hop_tc = cell_containing(&xml, "Hop");
        assert!(
            hop_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && hop_tc.contains(&format!(r#"w:color="{TH_RULE_COLOR}""#)),
            "slate grid must keep th teal fill: {hop_tc}"
        );
        let dock_p = para_containing(&xml, "Receiving dock is clear");
        assert_eq!(
            num_id_in(&dock_p),
            Some(TASK_CHECKED_NUM_ID),
            "slate grid must keep checked task numId 4: {dock_p}"
        );
        assert!(
            xml.contains("<a:blip r:embed="),
            "slate grid must keep 24pt a:blip: {xml}"
        );
        let emu = hu_to_emu(MIN_IMAGE_EDGE_HU);
        assert!(
            xml.contains(&format!(r#"<wp:extent cx="{emu}" cy="{emu}"/>"#)),
            "slate grid must keep the 24pt a:blip floor: {xml}"
        );

        let mut red = Table::from_cells(vec![vec!["Keep".into()]]);
        red.borders.top.color = Color::rgb(0xFF, 0, 0);
        red.borders.left.color = Color::rgb(0xFF, 0, 0);
        red.borders.bottom.color = Color::rgb(0xFF, 0, 0);
        red.borders.right.color = Color::rgb(0xFF, 0, 0);
        red.borders.inside_h.color = Color::rgb(0xFF, 0, 0);
        red.borders.inside_v.color = Color::rgb(0xFF, 0, 0);
        let mut red_doc = Document::new();
        let mut red_section = Section::default();
        red_section.body.push(Block::Table(red));
        red_doc.sections.push(red_section);
        let red_xml = document_xml(&red_doc);
        let red_grid = tbl_borders_block(&red_xml);
        assert!(
            red_grid.contains(r#"w:color="FF0000""#) && !red_grid.contains(r#"w:color="CBD5E1""#),
            "explicit non-black table borders must not remap to letter slate: {red_grid}"
        );
    }

    #[test]
    fn write_word_package_like_pandoc() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h1));
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![image_run(mark_png(), 2400, 2400, "Harbor mark")];
        section.body.push(Block::Paragraph(img_p));
        doc.sections.push(section);

        let xml = document_xml(&doc);
        assert!(
            xml.contains(r#"w:val="Heading1""#) && xml.contains("<w:tbl>"),
            "letter hop must keep Heading1 + table: {xml}"
        );
        assert!(
            xml.contains(tbl_cell_spacing_xml())
                && !xml.contains(r#"<w:tblCellSpacing w:w="40""#)
                && !xml.contains(r#"<w:tblCellSpacing w:w="20""#),
            "letter hop must collapse like letter.html, not Word 2003 2px gutters: {xml}"
        );
        assert!(
            xml.contains(r#"<a:graphic xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">"#)
                && xml.contains("<a:blip r:embed="),
            "python-docx xmlns:a on a:graphic so Word paints a:blip: {xml}"
        );
        let emu = hu_to_emu(MIN_IMAGE_EDGE_HU);
        assert!(
            xml.contains(&format!(r#"<wp:extent cx="{emu}" cy="{emu}"/>"#)),
            "Harbor a:blip must keep the 24pt floor: {xml}"
        );
        let table_grid = style_fragment(STYLES_XML, "TableGrid");
        assert!(
            table_grid.contains(tbl_cell_spacing_xml()),
            "TableGrid must keep collapse so style-only tables are not sparse: {table_grid}"
        );
        assert!(
            STYLES_XML.contains("<w:docDefaults>")
                && STYLES_XML.contains(r#"w:ascii="Segoe UI""#)
                && STYLES_XML.contains(r#"<w:color w:val="134E4A"/>"#)
                && STYLES_XML.contains(r#"w:line="276""#),
            "docDefaults must stay letter Segoe UI 11pt #134e4a line 276, not theme Calibri: {STYLES_XML}"
        );
        assert!(
            SETTINGS_XML.contains(r#"w:name="compatibilityMode""#)
                && SETTINGS_XML.contains(r#"w:val="15""#)
                && SETTINGS_XML.contains("overrideTableStyleFontSizeAndJustification")
                && SETTINGS_XML.contains(r#"w:val="doNotCompress""#),
            "settings must stay Word 2013 / python-docx so H1 tracking and table sz paint: {SETTINGS_XML}"
        );

        let bytes = write(&doc).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut settings = String::new();
        zip.by_name("word/settings.xml")
            .expect("word/settings.xml")
            .read_to_string(&mut settings)
            .unwrap();
        assert!(
            settings.contains(r#"w:val="15""#) && settings.contains("doNotCompress"),
            "package must store Word 2013 settings: {settings}"
        );
        let mut rels = String::new();
        zip.by_name("word/_rels/document.xml.rels")
            .expect("word/_rels/document.xml.rels")
            .read_to_string(&mut rels)
            .unwrap();
        assert!(
            rels.contains("/relationships/settings")
                && rels.contains("Target=\"settings.xml\"")
                && rels.contains("/relationships/image"),
            "settings rel must not steal the Harbor a:blip rId: {rels}"
        );
        assert!(
            rels.contains(r#"Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image""#),
            "Harbor a:blip must stay rId3: {rels}"
        );
        let hop_tc = cell_containing(&xml, "Hop");
        assert!(
            hop_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && hop_tc.contains(&format!(r#"w:color="{TH_RULE_COLOR}""#)),
            "word package must keep th teal fill: {hop_tc}"
        );
    }

    #[test]
    fn write_heading_rule_and_table_margin_like_word() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation — order 48291");
        h2.outline_level = Some(2);
        h2.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h2));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![image_run(mark_png(), 1200, 1200, "Harbor mark")];
        section.body.push(Block::Paragraph(img_p));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        let mut code = Paragraph::from_text("docagent prove examples/letter.md");
        code.code_block = true;
        code.space_after = 200;
        section.body.push(Block::Paragraph(code));
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        section
            .body
            .push(Block::Paragraph(task_item("Receiving dock is clear", true)));
        let mut super_p = Paragraph::from_text("A");
        super_p.runs[0].style.superscript = true;
        section.body.push(Block::Paragraph(super_p));
        let mut sub_p = Paragraph::from_text("2");
        sub_p.runs[0].style.subscript = true;
        section.body.push(Block::Paragraph(sub_p));
        doc.sections.push(section);

        let xml = document_xml(&doc);
        let h1_p = para_containing(&xml, r#"w:val="Heading1""#);
        let h2_p = para_containing(&xml, r#"w:val="Heading2""#);
        let h1_sz = hu_to_border_sz(HEADING1_RULE_HU);
        let h2_sz = hu_to_border_sz(HEADING2_RULE_HU);
        assert!(h1_sz >= 18 && HEADING1_RULE_HU == 225, "letter 3px is sz 18, got {h1_sz}");
        assert!(h2_sz >= 12 && HEADING2_RULE_HU == 150, "letter 2px is sz 12, got {h2_sz}");
        assert!(
            h1_p.contains("<w:pBdr>")
                && h1_p.contains("<w:bottom ")
                && h1_p.contains(&format!(r#"w:sz="{h1_sz}""#))
                && h1_p.contains(r#"w:color="134E4A""#)
                && p_bdr_bottom_sz(&h1_p) == Some(h1_sz),
            "H1 must emit Word bottom pBdr matching letter.html 3px #134e4a: {h1_p}"
        );
        assert!(
            h2_p.contains("<w:pBdr>")
                && h2_p.contains("<w:bottom ")
                && h2_p.contains(&format!(r#"w:sz="{h2_sz}""#))
                && h2_p.contains(&format!(r#"w:color="{TH_RULE_COLOR}""#))
                && p_bdr_bottom_sz(&h2_p) == Some(h2_sz),
            "H2 must emit Word bottom pBdr matching letter.html 2px #0d9488: {h2_p}"
        );
        let h1_style = style_fragment(STYLES_XML, "Heading1");
        let h2_style = style_fragment(STYLES_XML, "Heading2");
        assert!(
            h1_style.contains(r#"w:sz="18""#) && h1_style.contains(r#"w:color="134E4A""#),
            "Heading1 style must keep the 3px rule: {h1_style}"
        );
        assert!(
            h2_style.contains(r#"w:sz="12""#) && h2_style.contains(&format!(r#"w:color="{TH_RULE_COLOR}""#)),
            "Heading2 style must keep the 2px rule: {h2_style}"
        );

        let code_p = para_containing(&xml, "docagent prove");
        let table_gap = hu_to_twips(TABLE_MARGIN_HU);
        assert!(
            table_gap >= 240 && spacing_attr(&code_p, "w:after").unwrap_or(0) >= table_gap,
            "code_block before the hop table must keep letter 1rem ({table_gap} twips): {code_p}"
        );

        let quote_p = para_containing(&xml, "Replay is evidence");
        assert!(
            quote_p.contains("<w:pBdr>")
                && quote_p.contains("<w:left ")
                && p_bdr_left_sz(&quote_p).unwrap_or(0) >= 24
                && quote_p.contains(&format!(r#"w:color="{QUOTE_COLOR}""#)),
            "heading rule must keep quote 4px #134e4a left bar: {quote_p}"
        );
        let hop_tc = cell_containing(&xml, "Hop");
        assert!(
            hop_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && hop_tc.contains(&format!(r#"w:color="{TH_RULE_COLOR}""#))
                && hop_tc.contains("<w:tcBorders>"),
            "heading rule must keep th teal fill: {hop_tc}"
        );
        let dock_p = para_containing(&xml, "Receiving dock is clear");
        assert_eq!(
            num_id_attr(&dock_p),
            Some(TASK_CHECKED_NUM_ID),
            "heading rule must keep checked task numId 4: {dock_p}"
        );
        let a_run = run_containing(&xml, ">A</w:t>");
        let two_run = run_containing(&xml, ">2</w:t>");
        assert!(
            a_run.contains(r#"<w:vertAlign w:val="superscript"/>"#)
                && two_run.contains(r#"<w:vertAlign w:val="subscript"/>"#),
            "heading rule must keep super/sub vertAlign: {a_run} {two_run}"
        );
        let emu = hu_to_emu(MIN_IMAGE_EDGE_HU);
        assert!(
            xml.contains("<a:blip r:embed=")
                && xml.contains(&format!(r#"<wp:extent cx="{emu}" cy="{emu}"/>"#)),
            "heading rule must keep 24pt a:blip: {xml}"
        );
    }

    #[test]
    fn write_table_emits_cell_padding() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into(), "Proof".into()],
            vec!["Input".into(), "letter.md".into(), "input SHA-256".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let xml = document_xml(&doc);
        let x = hu_to_twips(CELL_PAD_X_HU);
        let y = hu_to_twips(CELL_PAD_Y_HU);
        assert!(x >= 156, "letter CSS 0.65rem is 156 twips, got {x}");
        assert!(y >= 96, "letter CSS 0.4rem is 96 twips, got {y}");
        assert!(
            xml.contains("<w:tblCellMar>"),
            "table must set Word cell padding, not an empty grid: {xml}"
        );
        assert!(
            xml.contains(&format!(r#"<w:left w:w="{x}" w:type="dxa"/>"#)),
            "tblCellMar left must be letter 0.65rem ({x} twips): {xml}"
        );
        assert!(
            xml.contains(&format!(r#"<w:right w:w="{x}" w:type="dxa"/>"#)),
            "tblCellMar right must be letter 0.65rem ({x} twips): {xml}"
        );
        assert!(
            xml.contains(&format!(r#"<w:top w:w="{y}" w:type="dxa"/>"#)),
            "tblCellMar top must be letter 0.4rem ({y} twips): {xml}"
        );
        assert!(
            xml.contains(&format!(r#"<w:bottom w:w="{y}" w:type="dxa"/>"#)),
            "tblCellMar bottom must be letter 0.4rem ({y} twips): {xml}"
        );
        assert!(
            xml.contains("<w:tcMar>"),
            "cells must carry w:tcMar so Word does not keep 0-pad TableGrid: {xml}"
        );
        assert!(
            !xml.contains(r#"<w:left w:w="0" w:type="dxa"/>"#),
            "cell padding must not collapse to 0: {xml}"
        );
    }

    #[test]
    fn write_table_header_fill_like_word() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into(), "Proof".into()],
            vec!["Input".into(), "letter.md".into(), "input SHA-256".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        let mut code = Paragraph::from_text("prove");
        code.runs[0].style.code = true;
        section.body.push(Block::Paragraph(code));
        let mut mark = Paragraph::from_text("three hashes");
        mark.runs[0].style.highlight = Some(docagent_model::Color::MARK);
        section.body.push(Block::Paragraph(mark));
        let mut task = Paragraph::from_text("Receiving dock is clear");
        task.numbering = Some(NumberingRef {
            definition_id: 2,
            level: 0,
            start: Some(1),
            format: Some(NumberFormat::Task),
        });
        section.body.push(Block::Paragraph(task));
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        section.body.push(Block::Paragraph(h1));
        doc.sections.push(section);
        let xml = document_xml(&doc);

        assert!(
            xml.contains("<w:tblHeader/>"),
            "Word header row must be w:tblHeader, not a body row: {xml}"
        );
        let hop_tc = cell_containing(&xml, "Hop");
        let file_tc = cell_containing(&xml, "File");
        let proof_tc = cell_containing(&xml, "Proof");
        let input_tc = cell_containing(&xml, "Input");
        let letter_tc = cell_containing(&xml, "letter.md");
        let rule_sz = hu_to_border_sz(TH_RULE_HU);
        assert!(rule_sz >= 12, "letter 2px is OOXML sz 12, got {rule_sz}");
        for (name, tc) in [("Hop", &hop_tc), ("File", &file_tc), ("Proof", &proof_tc)] {
            assert!(
                tc.contains(&format!(r#"w:fill="{TH_FILL}""#)),
                "{name} must keep WeasyPrint teal fill #ccfbf1, not a sparse grid: {tc}"
            );
            assert!(
                tc.contains("<w:tcBorders>")
                    && tc.contains(r#"<w:bottom "#)
                    && tc.contains(&format!(r#"w:sz="{rule_sz}""#))
                    && tc.contains(&format!(r#"w:color="{TH_RULE_COLOR}""#)),
                "{name} must keep 0.5pt grid + letter 2px #0d9488 header rule: {tc}"
            );
            assert!(
                !tc.contains(r#"w:fill="FFFFFF""#)
                    && !tc.contains(r#"w:fill="auto""#)
                    && !tc.contains(r#"w:val="nil""#),
                "UA/reset {name} is a sparse grid, not WeasyPrint letter input: {tc}"
            );
        }
        assert!(
            !input_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && !letter_tc.contains(&format!(r#"w:fill="{TH_FILL}""#)),
            "body td must not keep th fill: {input_tc} {letter_tc}"
        );
        let hop_p = para_containing(&xml, "Hop");
        assert!(
            hop_p.contains("<w:b/>") && hop_p.contains(&format!(r#"w:val="{TH_COLOR}""#)),
            "header Hop must keep letter teal bold: {hop_p}"
        );
        let input_p = para_containing(&xml, "Input");
        assert!(
            input_p.contains(&format!(r#"<w:color w:val="{BODY_COLOR}"/>"#))
                && !input_p.contains("<w:b/>")
                && !input_p.contains(r#"w:val="000000""#)
                && !input_p.contains(r#"w:color="auto""#),
            "body Input must stay letter teal type, not UA black or th bold: {input_p}"
        );

        let table_grid = style_fragment(STYLES_XML, "TableGrid");
        assert!(
            table_grid.contains(r#"w:type="firstRow""#)
                && table_grid.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && table_grid.contains(&format!(r#"w:color="{TH_RULE_COLOR}""#))
                && table_grid.contains(r#"w:sz="12""#),
            "TableGrid firstRow must keep letter th fill + 2px teal rule: {table_grid}"
        );
        assert!(
            table_grid.contains(&format!(r#"w:color="{TBL_BORDER_COLOR}""#))
                && table_grid.contains(r#"w:sz="6""#)
                && !table_grid.contains(r#"w:color="000000""#),
            "TableGrid must keep letter 1px #cbd5e1, not Word black hairline: {table_grid}"
        );
        assert!(
            hop_tc.contains(&format!(r#"<w:vAlign w:val="{CELL_VALIGN}"/>"#))
                && input_tc.contains(&format!(r#"<w:vAlign w:val="{CELL_VALIGN}"/>"#))
                && table_grid.contains(&format!(r#"<w:vAlign w:val="{CELL_VALIGN}"/>"#)),
            "th fill must keep letter vertical-align top: {hop_tc} {input_tc}"
        );

        let prove_p = para_containing(&xml, "prove");
        assert!(
            prove_p.contains("Consolas")
                && prove_p.contains(&format!(r#"w:fill="{CODE_FILL}""#)),
            "th fill must keep Code Consolas chip: {prove_p}"
        );
        let mark_p = para_containing(&xml, "three hashes");
        assert!(
            mark_p.contains(r#"<w:highlight w:val="yellow"/>"#)
                && mark_p.contains(&format!(r#"w:fill="{MARK_FILL}""#)),
            "th fill must keep Mark highlight: {mark_p}"
        );
        let dock_p = para_containing(&xml, "Receiving dock is clear");
        assert_eq!(
            num_id_attr(&dock_p),
            Some(TASK_CHECKED_NUM_ID),
            "th fill must keep checked task numId 4: {dock_p}"
        );
        let h1_p = para_containing(&xml, r#"w:val="Heading1""#);
        assert!(
            !h1_p.contains("<w:sz "),
            "th fill must keep Heading1 16pt style: {h1_p}"
        );
        assert_eq!(
            hu_to_emu(MIN_IMAGE_EDGE_HU),
            304_800,
            "th fill must keep 24pt a:blip floor"
        );
    }

    #[test]
    fn write_image_paragraph_spaces_after_mark() {
        let png = mark_png();
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![image_run(png, 1200, 1200, "Harbor mark")];
        img_p.space_before = 80;
        img_p.space_after = 200;
        section.body.push(Block::Paragraph(img_p));
        let mut body = Paragraph::from_text("6 September 2026");
        body.space_after = 200;
        section.body.push(Block::Paragraph(body));
        doc.sections.push(section);
        let xml = document_xml(&doc);
        let before = hu_to_twips(IMAGE_BEFORE_HU);
        let after = hu_to_twips(IMAGE_AFTER_HU);
        let body_after = hu_to_twips(BODY_AFTER_HU);
        assert_eq!(IMAGE_BEFORE_HU, 720);
        assert_eq!(IMAGE_AFTER_HU, 840);
        assert_eq!(before, 144, "letter CSS img 0.6rem is 144 twips, got {before}");
        assert_eq!(after, 168, "letter CSS 0.7rem is 168 twips, got {after}");
        assert!(
            IMAGE_BEFORE_HU < IMAGE_AFTER_HU && before < after,
            "img 0.6rem before must stay tighter than collapsed 0.7rem after"
        );
        assert_eq!(BODY_AFTER_HU, 840);
        assert_eq!(body_after, 168, "letter CSS p 0.7rem is 168 twips, got {body_after}");
        assert_eq!(
            IMAGE_AFTER_HU, BODY_AFTER_HU,
            "image 0.7rem must match body p 0.7rem"
        );
        let p = para_containing(&xml, "<a:blip r:embed=");
        assert_eq!(
            spacing_attr(&p, "w:before"),
            Some(before),
            "Harbor mark paragraph must space before {before} twips (0.6rem), not IR 80 HU: {p}"
        );
        assert_eq!(
            spacing_attr(&p, "w:after"),
            Some(after),
            "Harbor mark paragraph must space after {after} twips (0.7rem), not IR 200 HU: {p}"
        );
        assert!(
            !p.contains(r#"w:before="0""#)
                && !p.contains(r#"w:before="16""#)
                && !p.contains(r#"w:before="80""#),
            "Word 0 / IR 80 / H2 4pt before the mark is jammed vs WeasyPrint 0.6rem: {p}"
        );
        assert!(
            !p.contains(r#"w:after="160""#) && !p.contains(r#"w:after="240""#),
            "Word Normal 8pt / last-list 1rem after the mark is jammed or sparse vs WeasyPrint 0.7rem: {p}"
        );
        let body_p = para_containing(&xml, "6 September 2026");
        assert_eq!(
            spacing_attr(&body_p, "w:after"),
            Some(body_after),
            "after-mark 0.7rem must not leak onto the next body: {body_p}"
        );
        assert!(xml.contains("<a:blip r:embed="), "{xml}");
        let emu = hu_to_emu(MIN_IMAGE_EDGE_HU);
        assert!(
            xml.contains(&format!(r#"<wp:extent cx="{emu}" cy="{emu}"/>"#)),
            "Harbor a:blip must keep the 24pt floor: {xml}"
        );
    }

    fn style_fragment(xml: &str, style_id: &str) -> String {
        let needle = format!(r#"w:styleId="{style_id}""#);
        let start = xml.find(&needle).unwrap_or_else(|| panic!("{style_id}"));
        let rest = &xml[start..];
        let end = rest.find("</w:style>").expect("style end");
        rest[..end].to_string()
    }

    fn para_containing(xml: &str, needle: &str) -> String {
        let at = xml.find(needle).unwrap_or_else(|| panic!("{needle}"));
        let start = p_tag_start(&xml[..at]).expect("p start");
        let end = xml[at..].find("</w:p>").expect("p end");
        xml[start..at + end + 6].to_string()
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

    fn run_containing(xml: &str, needle: &str) -> String {
        let at = xml.find(needle).unwrap_or_else(|| panic!("{needle}"));
        let start = xml[..at]
            .rfind("<w:r>")
            .or_else(|| xml[..at].rfind("<w:r "))
            .expect("r start");
        let end = xml[at..].find("</w:r>").expect("r end");
        xml[start..at + end + 6].to_string()
    }

    fn cell_containing(xml: &str, needle: &str) -> String {
        let at = xml.find(needle).unwrap_or_else(|| panic!("{needle}"));
        let start = xml[..at]
            .rfind("<w:tc>")
            .or_else(|| xml[..at].rfind("<w:tc "))
            .expect("tc start");
        let end = xml[at..].find("</w:tc>").expect("tc end");
        xml[start..at + end + 7].to_string()
    }

    fn tbl_borders_block(xml: &str) -> String {
        let start = xml
            .find("<w:tblBorders>")
            .unwrap_or_else(|| panic!("tblBorders: {xml}"));
        let rest = &xml[start..];
        let end = rest.find("</w:tblBorders>").expect("tblBorders end");
        rest[..end].to_string()
    }

    fn num_id_attr(p: &str) -> Option<u32> {
        let start = p.find("<w:numId ")?;
        let tag = p[start..].split('>').next()?;
        let rest = tag.split(r#"w:val=""#).nth(1)?;
        rest.split('"').next()?.parse().ok()
    }

    fn spacing_attr(p: &str, key: &str) -> Option<i32> {
        let start = p.find("<w:spacing ")?;
        let tag = p[start..].split('>').next()?;
        let needle = format!("{key}=\"");
        let rest = tag.split(&needle).nth(1)?;
        rest.split('"').next()?.parse().ok()
    }

    fn pg_mar_attr(xml: &str, key: &str) -> Option<i32> {
        let start = xml.find("<w:pgMar ")?;
        let tag = xml[start..].split('>').next()?;
        let needle = format!("{key}=\"");
        let rest = tag.split(&needle).nth(1)?;
        rest.split('"').next()?.parse().ok()
    }

    fn ind_attr(p: &str, key: &str) -> Option<i32> {
        let start = p.find("<w:ind ")?;
        let tag = p[start..].split('>').next()?;
        let needle = format!("{key}=\"");
        let rest = tag.split(&needle).nth(1)?;
        rest.split('"').next()?.parse().ok()
    }

    fn p_bdr_left_sz(fragment: &str) -> Option<i32> {
        let start = fragment.find("<w:left ")?;
        let tag = fragment[start..].split('>').next()?;
        let rest = tag.split(r#"w:sz=""#).nth(1)?;
        rest.split('"').next()?.parse().ok()
    }

    fn p_bdr_bottom_sz(fragment: &str) -> Option<i32> {
        let start = fragment.find("<w:bottom ")?;
        let tag = fragment[start..].split('>').next()?;
        let rest = tag.split(r#"w:sz=""#).nth(1)?;
        rest.split('"').next()?.parse().ok()
    }

    fn p_bdr_top_sz(fragment: &str) -> Option<i32> {
        let start = fragment.find("<w:top ")?;
        let tag = fragment[start..].split('>').next()?;
        let rest = tag.split(r#"w:sz=""#).nth(1)?;
        rest.split('"').next()?.parse().ok()
    }

    fn bdr_space_for_color(fragment: &str, color: &str) -> Option<i32> {
        let mut rest = fragment;
        let color_needle = format!(r#"w:color="{color}""#);
        while let Some(start) = rest.find("<w:bdr ") {
            let tag = rest[start..].split('>').next()?;
            if tag.contains(&color_needle) {
                let needle = r#"w:space=""#;
                let val = tag.split(needle).nth(1)?.split('"').next()?;
                return val.parse().ok();
            }
            rest = &rest[start + 7..];
        }
        None
    }

    fn numbered_item(text: &str, level: u8, format: NumberFormat) -> Paragraph {
        let mut p = Paragraph::from_text(text);
        p.space_after = 80;
        p.numbering = Some(NumberingRef {
            definition_id: u32::from(level),
            level,
            start: None,
            format: Some(format),
        });
        p
    }

    fn style_sz(fragment: &str) -> i32 {
        let start = fragment.find("<w:sz ").expect("w:sz");
        let tag = fragment[start..].split('>').next().expect("sz tag");
        let rest = tag.split(r#"w:val=""#).nth(1).expect("sz val");
        rest.split('"').next().expect("sz close").parse().expect("sz int")
    }

    #[test]
    fn styles_xml_heading_size_and_spacing_match_word() {
        let h1 = style_fragment(STYLES_XML, "Heading1");
        let h2 = style_fragment(STYLES_XML, "Heading2");
        let normal = style_fragment(STYLES_XML, "Normal");
        assert!(
            style_sz(&h1) >= 32,
            "Heading1 must be Word 16pt (sz 32), not a sparse outline: {h1}"
        );
        assert!(
            spacing_attr(&h1, "w:before").unwrap_or(0) >= 240,
            "Heading1 before must be Word 12pt (240 twips): {h1}"
        );
        assert!(
            spacing_attr(&h1, "w:after").unwrap_or(0) >= 160,
            "Heading1 after must keep 8pt so H1+H2 is not stacked: {h1}"
        );
        assert!(
            style_sz(&h2) >= 26,
            "Heading2 must be Word 13pt (sz 26): {h2}"
        );
        assert!(
            spacing_attr(&h2, "w:before").unwrap_or(0) >= 200,
            "Heading2 before must be Word 10pt (200 twips), not 40: {h2}"
        );
        assert_eq!(
            spacing_attr(&normal, "w:after"),
            Some(hu_to_twips(BODY_AFTER_HU)),
            "Normal after must stay letter.css p 0.7rem (168 twips), not Word 8pt: {normal}"
        );
        assert!(
            normal.contains(&format!(r#"<w:color w:val="{BODY_COLOR}"/>"#))
                && !normal.contains(r#"w:val="000000""#)
                && !normal.contains(r#"w:color="auto""#),
            "Normal type must stay letter #134e4a, not UA black: {normal}"
        );
        assert!(
            spacing_attr(&normal, "w:line").unwrap_or(0) >= 276,
            "Normal line must be Word 1.15 (276): {normal}"
        );
        assert!(
            h1.contains("<w:pBdr>")
                && h1.contains("<w:bottom ")
                && h1.contains(r#"w:sz="18""#)
                && h1.contains(r#"w:color="134E4A""#),
            "Heading1 style must keep letter 3px #134e4a rule: {h1}"
        );
        assert!(
            h1.contains(&format!(r#"<w:color w:val="{HEADING_COLOR}"/>"#))
                && !h1.contains(r#"w:val="2F5496""#)
                && !h1.contains(r#"w:val="1F4D78""#),
            "Heading1 type must stay letter #134e4a, not Word Calibri blue: {h1}"
        );
        assert!(
            h2.contains("<w:pBdr>")
                && h2.contains("<w:bottom ")
                && h2.contains(r#"w:sz="12""#)
                && h2.contains(r#"w:color="0D9488""#),
            "Heading2 style must keep letter 2px #0d9488 rule: {h2}"
        );
        assert!(
            h2.contains(&format!(r#"<w:color w:val="{HEADING_COLOR}"/>"#))
                && !h2.contains(r#"w:val="2F5496""#),
            "Heading2 type must stay letter #134e4a, not Word Calibri blue: {h2}"
        );
        let bytes = write(&Document::new()).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut styles = String::new();
        zip.by_name("word/styles.xml")
            .unwrap()
            .read_to_string(&mut styles)
            .unwrap();
        let packed_h1 = style_fragment(&styles, "Heading1");
        assert!(
            style_sz(&packed_h1) >= 32,
            "packaged styles.xml must keep Word Heading1 size: {packed_h1}"
        );
        assert!(
            spacing_attr(&packed_h1, "w:before").unwrap_or(0) >= 240,
            "packaged styles.xml must keep Word Heading1 spacing: {packed_h1}"
        );
        assert!(
            packed_h1.contains("<w:pBdr>")
                && packed_h1.contains(r#"w:sz="18""#)
                && packed_h1.contains(r#"w:color="134E4A""#),
            "packaged Heading1 must keep letter 3px #134e4a rule: {packed_h1}"
        );
        let packed_h2 = style_fragment(&styles, "Heading2");
        assert!(
            packed_h2.contains("<w:pBdr>")
                && packed_h2.contains(r#"w:sz="12""#)
                && packed_h2.contains(r#"w:color="0D9488""#),
            "packaged Heading2 must keep letter 2px #0d9488 rule: {packed_h2}"
        );
        assert!(
            packed_h1.contains(&format!(r#"<w:color w:val="{HEADING_COLOR}"/>"#))
                && packed_h2.contains(&format!(r#"<w:color w:val="{HEADING_COLOR}"/>"#))
                && !styles.contains(r#"w:val="2F5496""#)
                && !styles.contains(r#"w:val="1F4D78""#),
            "packaged Heading1/2 type must stay letter #134e4a, not Word Calibri blue"
        );
    }

    #[test]
    fn write_heading_and_body_spacing_match_word() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.space_before = 200;
        h1.space_after = 400;
        h1.runs[0].style.bold = true;
        h1.runs[0].style.size = 1800;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation");
        h2.outline_level = Some(2);
        h2.space_before = 360;
        h2.space_after = 240;
        h2.runs[0].style.bold = true;
        h2.runs[0].style.size = 1400;
        section.body.push(Block::Paragraph(h2));
        let mut body = Paragraph::from_text("6 September 2026");
        body.space_after = 200;
        section.body.push(Block::Paragraph(body));
        let table = Table::from_cells(vec![vec!["Hop".into()]]);
        section.body.push(Block::Table(table));
        doc.sections.push(section);

        let xml = document_xml(&doc);
        let h1_p = para_containing(&xml, r#"w:val="Heading1""#);
        let h2_p = para_containing(&xml, r#"w:val="Heading2""#);
        let body_p = para_containing(&xml, "6 September 2026");
        let hop_p = para_containing(&xml, "Hop");
        let h1_before = hu_to_twips(HEADING1_BEFORE_HU);
        let h1_after = hu_to_twips(HEADING1_AFTER_HU);
        let h2_before = hu_to_twips(HEADING2_BEFORE_HU);
        let body_after = hu_to_twips(BODY_AFTER_HU);
        assert!(
            h1_before >= 240 && spacing_attr(&h1_p, "w:before").unwrap_or(0) >= h1_before,
            "H1 before must floor to Word 12pt ({h1_before} twips), not IR 40: {h1_p}"
        );
        assert!(
            h1_after >= 160 && spacing_attr(&h1_p, "w:after").unwrap_or(0) >= h1_after,
            "H1 after must floor to 8pt ({h1_after} twips): {h1_p}"
        );
        assert!(
            h2_before >= 200 && spacing_attr(&h2_p, "w:before").unwrap_or(0) >= h2_before,
            "H2 before must floor to Word 10pt ({h2_before} twips), not IR 72: {h2_p}"
        );
        assert!(
            body_after >= 168 && spacing_attr(&body_p, "w:after").unwrap_or(0) >= body_after,
            "body after must floor to letter.css 0.7rem ({body_after} twips), not IR 40: {body_p}"
        );
        let table_before = hu_to_twips(TABLE_MARGIN_HU);
        assert!(
            table_before >= 240 && spacing_attr(&body_p, "w:after").unwrap_or(0) >= table_before,
            "body before the hop table must keep letter 1rem ({table_before} twips), not jammed 8pt: {body_p}"
        );
        assert!(
            h1_p.contains("<w:pBdr>")
                && h1_p.contains("<w:bottom ")
                && p_bdr_bottom_sz(&h1_p).unwrap_or(0) >= 18
                && h1_p.contains(r#"w:color="134E4A""#),
            "H1 must keep letter 3px #134e4a bottom rule: {h1_p}"
        );
        assert!(
            h2_p.contains("<w:pBdr>")
                && h2_p.contains("<w:bottom ")
                && p_bdr_bottom_sz(&h2_p).unwrap_or(0) >= 12
                && h2_p.contains(r#"w:color="0D9488""#),
            "H2 must keep letter 2px #0d9488 bottom rule: {h2_p}"
        );
        assert!(
            spacing_attr(&body_p, "w:line").unwrap_or(0) >= BODY_LINE,
            "body line must be Word 1.15 ({BODY_LINE}): {body_p}"
        );
        assert!(
            spacing_attr(&hop_p, "w:after").unwrap_or(0) < body_after,
            "table cells must not inherit body 0.7rem after: {hop_p}"
        );
        assert!(
            !h1_p.contains("<w:sz "),
            "Heading1 run must not override Word 16pt style: {h1_p}"
        );
        assert!(
            !h2_p.contains("<w:sz "),
            "Heading2 run must not override Word 13pt style: {h2_p}"
        );
        assert!(
            body_p.contains(&format!(r#"<w:color w:val="{BODY_COLOR}"/>"#))
                && !body_p.contains(r#"w:val="000000""#)
                && !body_p.contains(r#"w:color="auto""#),
            "body run must stay letter #134e4a, not UA black: {body_p}"
        );
        let h1_run = run_containing(&h1_p, ">Northwind Freight</w:t>");
        let h2_run = run_containing(&h2_p, ">Delivery confirmation</w:t>");
        assert!(
            h1_run.contains(&format!(r#"<w:color w:val="{HEADING_COLOR}"/>"#))
                && !h1_run.contains(r#"w:val="2F5496""#)
                && !h1_run.contains(r#"w:val="000000""#),
            "H1 run must stay letter #134e4a, not Word Calibri blue: {h1_run}"
        );
        assert!(
            h2_run.contains(&format!(r#"<w:color w:val="{HEADING_COLOR}"/>"#))
                && !h2_run.contains(r#"w:val="2F5496""#),
            "H2 run must stay letter #134e4a, not Word Calibri blue: {h2_run}"
        );
    }

    #[test]
    fn write_heading_color_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation — order 48291");
        h2.outline_level = Some(2);
        h2.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h2));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        section
            .body
            .push(Block::Paragraph(task_item("Receiving dock is clear", true)));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![image_run(mark_png(), 2400, 2400, "Harbor mark")];
        section.body.push(Block::Paragraph(img_p));
        doc.sections.push(section);

        let xml = document_xml(&doc);
        let h1_p = para_containing(&xml, r#"w:val="Heading1""#);
        let h2_p = para_containing(&xml, r#"w:val="Heading2""#);
        let h1_run = run_containing(&h1_p, ">Northwind Freight</w:t>");
        let h2_run = run_containing(&h2_p, ">Delivery confirmation — order 48291</w:t>");
        assert_eq!(HEADING_COLOR, BODY_COLOR);
        assert!(
            h1_run.contains(&format!(r#"<w:color w:val="{HEADING_COLOR}"/>"#))
                && !h1_run.contains(r#"w:val="2F5496""#)
                && !h1_run.contains(r#"w:val="000000""#)
                && !h1_run.contains(r#"w:color="auto""#),
            "H1 run must be letter #134e4a like body/quote/th, not Word 2F5496: {h1_run}"
        );
        assert!(
            h2_run.contains(&format!(r#"<w:color w:val="{HEADING_COLOR}"/>"#))
                && !h2_run.contains(r#"w:val="2F5496""#),
            "H2 run must be letter #134e4a, not Word Calibri blue: {h2_run}"
        );
        assert!(
            !h1_p.contains("<w:sz ") && !h2_p.contains("<w:sz "),
            "heading color must not reintroduce markdown CSS w:sz: {h1_p} {h2_p}"
        );
        let h1_style = style_fragment(STYLES_XML, "Heading1");
        let h2_style = style_fragment(STYLES_XML, "Heading2");
        assert!(
            h1_style.contains(&format!(r#"<w:color w:val="{HEADING_COLOR}"/>"#))
                && h2_style.contains(&format!(r#"<w:color w:val="{HEADING_COLOR}"/>"#))
                && !h1_style.contains(r#"w:val="2F5496""#)
                && !h2_style.contains(r#"w:val="2F5496""#),
            "Heading1/2 style type must stay letter #134e4a: {h1_style} {h2_style}"
        );

        let body_run = run_containing(&xml, ">6 September 2026</w:t>");
        assert!(
            body_run.contains(&format!(r#"<w:color w:val="{BODY_COLOR}"/>"#)),
            "heading color must keep body #134e4a: {body_run}"
        );
        let quote_p = para_containing(&xml, "Replay is evidence");
        assert!(
            quote_p.contains("<w:pBdr>")
                && quote_p.contains("<w:left ")
                && p_bdr_left_sz(&quote_p).unwrap_or(0) >= 24
                && quote_p.contains(&format!(r#"w:color="{QUOTE_COLOR}""#)),
            "heading color must keep quote 4px #134e4a left bar: {quote_p}"
        );
        let hop_tc = cell_containing(&xml, "Hop");
        assert!(
            hop_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && hop_tc.contains(&format!(r#"w:color="{TH_RULE_COLOR}""#))
                && hop_tc.contains("<w:tcBorders>"),
            "heading color must keep th teal fill: {hop_tc}"
        );
        let dock_p = para_containing(&xml, "Receiving dock is clear");
        assert_eq!(
            num_id_in(&dock_p),
            Some(TASK_CHECKED_NUM_ID),
            "heading color must keep checked task numId 4: {dock_p}"
        );
        assert!(
            xml.contains("<a:blip r:embed="),
            "heading color must keep 24pt a:blip: {xml}"
        );
        let emu = hu_to_emu(MIN_IMAGE_EDGE_HU);
        assert!(
            xml.contains(&format!(r#"<wp:extent cx="{emu}" cy="{emu}"/>"#)),
            "heading color must keep the 24pt a:blip floor: {xml}"
        );
    }

    #[test]
    fn write_heading_tracking_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation — order 48291");
        h2.outline_level = Some(2);
        h2.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h2));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        section
            .body
            .push(Block::Paragraph(task_item("Receiving dock is clear", true)));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![image_run(mark_png(), 2400, 2400, "Harbor mark")];
        section.body.push(Block::Paragraph(img_p));
        doc.sections.push(section);

        let xml = document_xml(&doc);
        let h1_p = para_containing(&xml, r#"w:val="Heading1""#);
        let h2_p = para_containing(&xml, r#"w:val="Heading2""#);
        let h1_run = run_containing(&h1_p, ">Northwind Freight</w:t>");
        let h2_run = run_containing(&h2_p, ">Delivery confirmation — order 48291</w:t>");
        assert_eq!(HEADING1_SPACING, -6);
        assert!(
            h1_run.contains(&format!(r#"<w:spacing w:val="{HEADING1_SPACING}"/>"#))
                && !h1_run.contains(r#"<w:spacing w:val="0"/>"#),
            "H1 run must keep letter.css -.02em tracking (w:spacing -6 at 16pt), not 0: {h1_run}"
        );
        assert!(
            !h2_run.contains("<w:spacing ") && !h2_p.contains(r#"<w:spacing w:val="#),
            "letter.css letter-spacing is h1-only, not Heading 2: {h2_run}"
        );
        assert!(
            !h1_p.contains("<w:sz ") && !h2_p.contains("<w:sz "),
            "h1 tracking must not reintroduce markdown CSS w:sz: {h1_p} {h2_p}"
        );
        let h1_style = style_fragment(STYLES_XML, "Heading1");
        let h2_style = style_fragment(STYLES_XML, "Heading2");
        let h3_style = style_fragment(STYLES_XML, "Heading3");
        assert!(
            h1_style.contains(&format!(r#"<w:spacing w:val="{HEADING1_SPACING}"/>"#))
                && !h1_style.contains(r#"<w:spacing w:val="0"/>"#),
            "Heading1 style must keep letter.css -.02em tracking: {h1_style}"
        );
        assert!(
            !h2_style.contains(r#"<w:spacing w:val="#)
                && !h3_style.contains(r#"<w:spacing w:val="#),
            "Heading2/3 must not pick up h1 tracking: {h2_style} {h3_style}"
        );
        assert!(
            h1_run.contains(&format!(r#"<w:color w:val="{HEADING_COLOR}"/>"#))
                && h1_style.contains(&format!(r#"<w:color w:val="{HEADING_COLOR}"/>"#)),
            "h1 tracking must keep heading letter teal: {h1_run} {h1_style}"
        );
        assert!(
            h1_p.contains("<w:pBdr>")
                && p_bdr_bottom_sz(&h1_p).unwrap_or(0) >= 18
                && h1_p.contains(r#"w:color="134E4A""#),
            "h1 tracking must keep letter 3px #134e4a rule: {h1_p}"
        );

        let body_run = run_containing(&xml, ">6 September 2026</w:t>");
        assert!(
            body_run.contains(&format!(r#"<w:color w:val="{BODY_COLOR}"/>"#))
                && !body_run.contains("<w:spacing "),
            "h1 tracking must keep body #134e4a and not leak onto Normal: {body_run}"
        );
        let quote_p = para_containing(&xml, "Replay is evidence");
        assert!(
            quote_p.contains("<w:pBdr>")
                && quote_p.contains("<w:left ")
                && p_bdr_left_sz(&quote_p).unwrap_or(0) >= 24
                && quote_p.contains(&format!(r#"w:color="{QUOTE_COLOR}""#)),
            "h1 tracking must keep quote 4px #134e4a left bar: {quote_p}"
        );
        let hop_tc = cell_containing(&xml, "Hop");
        assert!(
            hop_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && hop_tc.contains(&format!(r#"w:color="{TH_RULE_COLOR}""#))
                && hop_tc.contains("<w:tcBorders>"),
            "h1 tracking must keep th teal fill: {hop_tc}"
        );
        let hop_run = run_containing(&xml, ">Hop</w:t>");
        assert_eq!(
            style_sz(&hop_run),
            CELL_SZ,
            "h1 tracking must keep letter 0.95rem table type: {hop_run}"
        );
        let dock_p = para_containing(&xml, "Receiving dock is clear");
        assert_eq!(
            num_id_in(&dock_p),
            Some(TASK_CHECKED_NUM_ID),
            "h1 tracking must keep checked task numId 4: {dock_p}"
        );
        assert!(
            xml.contains("<a:blip r:embed="),
            "h1 tracking must keep 24pt a:blip: {xml}"
        );
        let emu = hu_to_emu(MIN_IMAGE_EDGE_HU);
        assert!(
            xml.contains(&format!(r#"<wp:extent cx="{emu}" cy="{emu}"/>"#)),
            "h1 tracking must keep the 24pt a:blip floor: {xml}"
        );
    }

    #[test]
    fn write_heading_leading_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation — order 48291");
        h2.outline_level = Some(2);
        h2.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h2));
        let mut h3 = Paragraph::from_text("Seals");
        h3.outline_level = Some(3);
        h3.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h3));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        section
            .body
            .push(Block::Paragraph(task_item("Receiving dock is clear", true)));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![image_run(mark_png(), 2400, 2400, "Harbor mark")];
        section.body.push(Block::Paragraph(img_p));
        doc.sections.push(section);

        assert_eq!(HEADING_LINE, 288);
        assert_eq!(BODY_LINE, 276);
        assert_eq!(HEADING1_SPACING, -6);

        let xml = document_xml(&doc);
        let h1_p = para_containing(&xml, r#"w:val="Heading1""#);
        let h2_p = para_containing(&xml, r#"w:val="Heading2""#);
        let h3_p = para_containing(&xml, r#"w:val="Heading3""#);
        assert_eq!(
            spacing_attr(&h1_p, "w:line"),
            Some(HEADING_LINE),
            "H1 must keep letter.css line-height:1.2 (w:line 288), not body 276: {h1_p}"
        );
        assert_eq!(
            spacing_attr(&h2_p, "w:line"),
            Some(HEADING_LINE),
            "H2 must keep letter.css line-height:1.2 (w:line 288), not body 276: {h2_p}"
        );
        assert_eq!(
            spacing_attr(&h3_p, "w:line"),
            Some(HEADING_LINE),
            "H3 must keep letter heading 1.2 leading: {h3_p}"
        );
        assert!(
            h1_p.contains(r#"w:lineRule="auto""#)
                && h2_p.contains(r#"w:lineRule="auto""#)
                && !h1_p.contains(r#"w:line="276""#)
                && !h2_p.contains(r#"w:line="276""#)
                && !h3_p.contains(r#"w:line="276""#)
                && !h1_p.contains(r#"w:line="240""#),
            "inherited Normal 1.15 / single is jammed heading leading vs WeasyPrint: {h1_p}"
        );
        let h1_run = run_containing(&h1_p, ">Northwind Freight</w:t>");
        let h2_run = run_containing(&h2_p, ">Delivery confirmation — order 48291</w:t>");
        assert!(
            h1_run.contains(&format!(r#"<w:spacing w:val="{HEADING1_SPACING}"/>"#)),
            "h1 leading must keep letter.css -.02em tracking: {h1_run}"
        );
        assert!(
            !h2_run.contains("<w:spacing ") && !h2_p.contains(r#"<w:spacing w:val="#),
            "letter.css letter-spacing is h1-only, not Heading 2: {h2_run}"
        );
        assert!(
            !h1_p.contains("<w:sz ") && !h2_p.contains("<w:sz ") && !h3_p.contains("<w:sz "),
            "h1 leading must not reintroduce markdown CSS w:sz: {h1_p} {h2_p} {h3_p}"
        );
        let h1_style = style_fragment(STYLES_XML, "Heading1");
        let h2_style = style_fragment(STYLES_XML, "Heading2");
        let h3_style = style_fragment(STYLES_XML, "Heading3");
        assert_eq!(
            spacing_attr(&h1_style, "w:line"),
            Some(HEADING_LINE),
            "Heading1 style must keep letter.css line-height:1.2: {h1_style}"
        );
        assert_eq!(
            spacing_attr(&h2_style, "w:line"),
            Some(HEADING_LINE),
            "Heading2 style must keep letter.css line-height:1.2: {h2_style}"
        );
        assert_eq!(
            spacing_attr(&h3_style, "w:line"),
            Some(HEADING_LINE),
            "Heading3 style must keep letter heading 1.2 leading: {h3_style}"
        );
        assert!(
            h1_style.contains(&format!(r#"<w:spacing w:val="{HEADING1_SPACING}"/>"#))
                && h1_style.contains(&format!(r#"<w:color w:val="{HEADING_COLOR}"/>"#)),
            "Heading1 style must keep letter tracking + teal: {h1_style}"
        );
        assert!(
            !h2_style.contains(r#"<w:spacing w:val="#)
                && !h3_style.contains(r#"<w:spacing w:val="#),
            "Heading2/3 must not pick up h1 tracking: {h2_style} {h3_style}"
        );

        let body_p = para_containing(&xml, "6 September 2026");
        let body_run = run_containing(&body_p, ">6 September 2026</w:t>");
        assert_eq!(
            spacing_attr(&body_p, "w:line"),
            Some(BODY_LINE),
            "body must keep Word 1.15 ({BODY_LINE}), not heading 1.2: {body_p}"
        );
        assert!(
            body_run.contains(&format!(r#"<w:color w:val="{BODY_COLOR}"/>"#))
                && !body_p.contains(r#"w:line="288""#),
            "h1 leading must keep body #134e4a and not leak 1.2 onto Normal: {body_run}"
        );
        let normal = style_fragment(STYLES_XML, "Normal");
        assert_eq!(
            spacing_attr(&normal, "w:line"),
            Some(BODY_LINE),
            "Normal must stay Word 1.15, not heading 1.2: {normal}"
        );
        let quote_p = para_containing(&xml, "Replay is evidence");
        assert_eq!(
            spacing_attr(&quote_p, "w:line"),
            Some(BODY_LINE),
            "quote must keep Word 1.15, not heading 1.2: {quote_p}"
        );
        assert!(
            quote_p.contains("<w:pBdr>")
                && quote_p.contains("<w:left ")
                && p_bdr_left_sz(&quote_p).unwrap_or(0) >= 24
                && quote_p.contains(&format!(r#"w:color="{QUOTE_COLOR}""#)),
            "h1 leading must keep quote 4px #134e4a left bar: {quote_p}"
        );
        let hop_tc = cell_containing(&xml, "Hop");
        assert!(
            hop_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && hop_tc.contains(&format!(r#"w:color="{TH_RULE_COLOR}""#))
                && hop_tc.contains("<w:tcBorders>"),
            "h1 leading must keep th teal fill: {hop_tc}"
        );
        let hop_run = run_containing(&xml, ">Hop</w:t>");
        assert_eq!(
            style_sz(&hop_run),
            CELL_SZ,
            "h1 leading must keep letter 0.95rem table type: {hop_run}"
        );
        let dock_p = para_containing(&xml, "Receiving dock is clear");
        assert_eq!(
            num_id_in(&dock_p),
            Some(TASK_CHECKED_NUM_ID),
            "h1 leading must keep checked task numId 4: {dock_p}"
        );
        assert!(
            xml.contains("<a:blip r:embed="),
            "h1 leading must keep 24pt a:blip: {xml}"
        );
        let emu = hu_to_emu(MIN_IMAGE_EDGE_HU);
        assert!(
            xml.contains(&format!(r#"<wp:extent cx="{emu}" cy="{emu}"/>"#)),
            "h1 leading must keep the 24pt a:blip floor: {xml}"
        );
        assert!(
            h1_p.contains("<w:pBdr>")
                && p_bdr_bottom_sz(&h1_p).unwrap_or(0) >= 18
                && h1_p.contains(r#"w:color="134E4A""#),
            "h1 leading must keep letter 3px #134e4a rule: {h1_p}"
        );
        assert!(
            h2_p.contains("<w:pBdr>")
                && p_bdr_bottom_sz(&h2_p).unwrap_or(0) >= 12
                && h2_p.contains(r#"w:color="0D9488""#),
            "h1 leading must keep letter 2px #0d9488 H2 rule: {h2_p}"
        );
    }

    #[test]
    fn write_letter_heading_uses_word_style_size() {
        // letter.md CSS is 1.75rem/1.15rem (2100/1380 HU). Word Heading1/2 is 16pt/13pt.
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.bold = true;
        h1.runs[0].style.size = 2100;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation — order 48291");
        h2.outline_level = Some(2);
        h2.runs[0].style.bold = true;
        h2.runs[0].style.size = 1380;
        section.body.push(Block::Paragraph(h2));
        doc.sections.push(section);
        let xml = document_xml(&doc);
        let h1_p = para_containing(&xml, r#"w:val="Heading1""#);
        let h2_p = para_containing(&xml, r#"w:val="Heading2""#);
        assert!(
            !h1_p.contains("<w:sz "),
            "letter H1 must use Word Heading1 16pt, not CSS 21pt: {h1_p}"
        );
        assert!(
            !h2_p.contains("<w:sz "),
            "letter H2 must use Word Heading2 13pt, not CSS 13.5pt: {h2_p}"
        );
        assert!(
            h1_p.contains("<w:pBdr>")
                && p_bdr_bottom_sz(&h1_p).unwrap_or(0) >= 18
                && h1_p.contains(r#"w:color="134E4A""#),
            "letter H1 must keep 3px #134e4a rule, not a missing Word heading: {h1_p}"
        );
        assert!(
            h2_p.contains("<w:pBdr>")
                && p_bdr_bottom_sz(&h2_p).unwrap_or(0) >= 12
                && h2_p.contains(r#"w:color="0D9488""#),
            "letter H2 must keep 2px #0d9488 rule: {h2_p}"
        );
        assert!(h1_p.contains("<w:b/>"), "letter H1 stays bold: {h1_p}");
        assert!(h2_p.contains("<w:b/>"), "letter H2 stays bold: {h2_p}");
        let h1_style = style_fragment(STYLES_XML, "Heading1");
        let h2_style = style_fragment(STYLES_XML, "Heading2");
        assert_eq!(style_sz(&h1_style), 32, "{h1_style}");
        assert_eq!(style_sz(&h2_style), 26, "{h2_style}");
        assert_eq!(hu_to_half_points(HEADING1_SIZE_HU), 32);
        assert_eq!(hu_to_half_points(HEADING2_SIZE_HU), 26);
    }

    #[test]
    fn styles_and_numbering_list_quote_code_match_word() {
        let quote = style_fragment(STYLES_XML, "Quote");
        let code = style_fragment(STYLES_XML, "CodeBlock");
        let list = style_fragment(STYLES_XML, "ListParagraph");
        let quote_before = spacing_attr(&quote, "w:before").unwrap_or(0);
        let quote_after = spacing_attr(&quote, "w:after").unwrap_or(0);
        assert!(
            (120..=240).contains(&quote_before),
            "Quote before must be Word/letter 6–12pt, not jammed or sparse: {quote}"
        );
        assert!(
            (160..=240).contains(&quote_after),
            "Quote after must be Word 8–12pt, not jammed IR 40 twips: {quote}"
        );
        assert!(
            ind_attr(&quote, "w:left").unwrap_or(0) >= QUOTE_LEFT_TWIPS,
            "Quote left indent must be Word 0.5in ({QUOTE_LEFT_TWIPS} twips): {quote}"
        );
        let quote_bar_sz = hu_to_border_sz(QUOTE_BAR_HU);
        assert!(
            quote_bar_sz >= 24 && QUOTE_BAR_HU == 300,
            "letter 4px is OOXML sz 24 / 300 HU, got {quote_bar_sz}"
        );
        assert!(
            quote.contains("<w:pBdr>")
                && quote.contains("<w:left ")
                && quote.contains(r#"w:sz="24""#)
                && quote.contains(&format!(r#"w:color="{QUOTE_COLOR}""#))
                && p_bdr_left_sz(&quote).unwrap_or(0) >= 24,
            "Quote style must keep letter.html 4px #134e4a left pBdr, not Word auto/gray: {quote}"
        );
        assert!(
            quote.contains(&format!(r#"<w:color w:val="{QUOTE_COLOR}"/>"#)),
            "Quote text must stay letter.html #134e4a like the 4px bar: {quote}"
        );
        assert!(
            !quote.contains(r#"w:val="nil""#)
                && !quote.contains(r#"w:color="auto""#)
                && !quote.contains(r#"w:color="CCC""#)
                && !quote.contains(r#"w:color="999999""#)
                && !quote.contains(r#"w:sz="6""#)
                && !quote.contains(r#"w:sz="8""#),
            "UA/reset/hairline/gray quote bar is not WeasyPrint letter input: {quote}"
        );
        let code_before = spacing_attr(&code, "w:before").unwrap_or(0);
        let code_after = spacing_attr(&code, "w:after").unwrap_or(0);
        assert_eq!(
            code_before,
            hu_to_twips(CODE_BEFORE_HU),
            "CodeBlock before must stay letter.css 0.6rem, not Word 6pt: {code}"
        );
        assert_eq!(
            code_after,
            hu_to_twips(CODE_AFTER_HU),
            "CodeBlock after must stay letter.css 1rem, not Word Normal 8pt: {code}"
        );
        assert_eq!(
            (ind_attr(&code, "w:left"), ind_attr(&code, "w:right")),
            (Some(hu_to_twips(CODE_PAD_X_HU)), Some(hu_to_twips(CODE_PAD_X_HU))),
            "CodeBlock must keep letter pre 1rem pad: {code}"
        );
        let y_sz = hu_to_border_sz(CODE_PAD_Y_HU);
        assert!(
            code.contains("<w:pBdr>")
                && p_bdr_top_sz(&code) == Some(y_sz)
                && p_bdr_bottom_sz(&code) == Some(y_sz)
                && code.contains(&format!(r#"w:color="{CODE_FILL}""#)),
            "CodeBlock must keep letter pre 0.75rem Y pad: {code}"
        );
        assert_eq!(
            ind_attr(&list, "w:left"),
            Some(LIST_LEFT_TWIPS),
            "ListParagraph must keep letter 1.25rem left, not Word 0.5in: {list}"
        );
        assert_eq!(
            spacing_attr(&list, "w:after"),
            Some(hu_to_twips(LIST_ITEM_AFTER_HU)),
            "ListParagraph after must be letter 0.35rem, not Word contextualSpacing 0: {list}"
        );
        assert!(
            !list.contains("<w:contextualSpacing"),
            "Word contextualSpacing jams consecutive li vs WeasyPrint 0.35rem: {list}"
        );
        let hr = style_fragment(STYLES_XML, "HorizontalLine");
        assert!(
            hr.contains(r#"w:sz="18""#) && hr.contains(r#"w:color="134E4A""#),
            "HorizontalLine must stay letter 3px teal, not Word hairline 6: {hr}"
        );
        assert!(
            spacing_attr(&hr, "w:before").unwrap_or(0) >= 264
                && spacing_attr(&hr, "w:after").unwrap_or(0) >= 264,
            "HorizontalLine must stay letter 1.1rem: {hr}"
        );
        let link = style_fragment(STYLES_XML, "Hyperlink");
        assert!(
            link.contains(r#"w:val="0D9488""#) && link.contains("<w:u "),
            "Hyperlink style must stay letter teal + underline: {link}"
        );
        let strike_style = style_fragment(STYLES_XML, "Strike");
        assert!(
            strike_style.contains("<w:strike/>") && !strike_style.contains(r#"w:val="none""#),
            "Strike style must stay Word strikethrough: {strike_style}"
        );
        let under_style = style_fragment(STYLES_XML, "Underline");
        assert!(
            under_style.contains(r#"<w:u w:val="single"/>"#)
                && !under_style.contains(r#"w:val="none""#),
            "Underline style must stay Word Single: {under_style}"
        );
        let super_style = style_fragment(STYLES_XML, "Superscript");
        assert!(
            super_style.contains(r#"<w:vertAlign w:val="superscript"/>"#)
                && !super_style.contains(r#"w:val="baseline""#)
                && !super_style.contains(r#"w:val="subscript""#),
            "Superscript style must stay Word Font Effects Super: {super_style}"
        );
        let sub_style = style_fragment(STYLES_XML, "Subscript");
        assert!(
            sub_style.contains(r#"<w:vertAlign w:val="subscript"/>"#)
                && !sub_style.contains(r#"w:val="baseline""#)
                && !sub_style.contains(r#"w:val="superscript""#),
            "Subscript style must stay Word Font Effects Sub: {sub_style}"
        );
        let code_span = style_fragment(STYLES_XML, "Code");
        assert!(
            code_span.contains("Consolas")
                && code_span.contains(&format!(r#"w:val="{CODE_COLOR}""#))
                && code_span.contains(&format!(r#"w:fill="{CODE_FILL}""#)),
            "Code character style must stay letter teal Consolas chip: {code_span}"
        );
        assert!(
            bdr_space_for_color(&code_span, CODE_FILL).unwrap_or(0) >= CODE_BDR_SPACE_PT,
            "Code character style must keep letter .1em/.35em pad: {code_span}"
        );
        let mark = style_fragment(STYLES_XML, "Mark");
        assert!(
            mark.contains(r#"w:val="yellow""#)
                && mark.contains(&format!(r#"w:fill="{MARK_FILL}""#))
                && mark.contains(&format!(r#"<w:color w:val="{MARK_COLOR}"/>"#))
                && !mark.contains(r#"w:val="000000""#),
            "Mark character style must stay Word highlight + letter teal on amber: {mark}"
        );
        assert!(
            bdr_space_for_color(&mark, MARK_FILL).unwrap_or(0) >= MARK_BDR_SPACE_PT,
            "Mark character style must keep letter .05em/.2em pad: {mark}"
        );
        let defs = numbering_xml();
        let lvl0 = format!(r#"w:left="{LIST_LEFT_TWIPS}" w:hanging="{LIST_HANGING_TWIPS}""#);
        let lvl1 = format!(
            r#"w:left="{}" w:hanging="{LIST_HANGING_TWIPS}""#,
            LIST_LEFT_TWIPS * 2
        );
        let lvl2 = format!(
            r#"w:left="{}" w:hanging="{LIST_HANGING_TWIPS}""#,
            LIST_LEFT_TWIPS * 3
        );
        assert!(
            defs.contains(&lvl0),
            "level 0 must be letter 1.25rem/1rem hanging, not Word 0.5in: {defs}"
        );
        assert!(
            defs.contains(&lvl1),
            "level 1 must be nested 2.5rem hanging, not Word 1.0in: {defs}"
        );
        assert!(
            defs.contains(&lvl2),
            "level 2 must be nested 3.75rem hanging, not Word 1.5in: {defs}"
        );
        assert!(
            !defs.contains(r#"w:left="720" w:hanging="360""#)
                && !defs.contains(r#"w:left="1440" w:hanging="360""#),
            "Word 0.5in/0.25in hanging is a sparse gutter vs WeasyPrint 1.25rem: {defs}"
        );
        assert!(
            defs.contains(&lvl0)
                && defs.contains(TASK_UNCHECKED_GLYPH)
                && defs.contains(TASK_CHECKED_GLYPH)
                && defs.contains("Segoe UI Symbol"),
            "task numbering must keep letter hanging indent and ☐/☒: {defs}"
        );

        let bytes = write(&Document::new()).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut styles = String::new();
        zip.by_name("word/styles.xml")
            .unwrap()
            .read_to_string(&mut styles)
            .unwrap();
        let mut numbering = String::new();
        zip.by_name("word/numbering.xml")
            .unwrap()
            .read_to_string(&mut numbering)
            .unwrap();
        let packed_quote = style_fragment(&styles, "Quote");
        assert!(
            ind_attr(&packed_quote, "w:left").unwrap_or(0) >= QUOTE_LEFT_TWIPS,
            "packaged Quote indent must stay Word 0.5in: {packed_quote}"
        );
        assert!(
            packed_quote.contains("<w:pBdr>")
                && packed_quote.contains("<w:left ")
                && packed_quote.contains(r#"w:sz="24""#)
                && packed_quote.contains(&format!(r#"w:color="{QUOTE_COLOR}""#))
                && packed_quote.contains(&format!(r#"<w:color w:val="{QUOTE_COLOR}"/>"#)),
            "packaged Quote must keep letter 4px #134e4a left bar: {packed_quote}"
        );
        let packed_hr = style_fragment(&styles, "HorizontalLine");
        assert!(
            packed_hr.contains(r#"w:sz="18""#)
                && spacing_attr(&packed_hr, "w:before").unwrap_or(0) >= 264,
            "packaged HorizontalLine must stay 3px / 1.1rem: {packed_hr}"
        );
        let packed_link = style_fragment(&styles, "Hyperlink");
        assert!(
            packed_link.contains(r#"w:val="0D9488""#) && packed_link.contains("<w:u "),
            "packaged Hyperlink must stay teal + underline: {packed_link}"
        );
        let packed_strike = style_fragment(&styles, "Strike");
        assert!(
            packed_strike.contains("<w:strike/>"),
            "packaged Strike must stay Word strikethrough: {packed_strike}"
        );
        let packed_under = style_fragment(&styles, "Underline");
        assert!(
            packed_under.contains(r#"<w:u w:val="single"/>"#),
            "packaged Underline must stay Word Single: {packed_under}"
        );
        let packed_super = style_fragment(&styles, "Superscript");
        assert!(
            packed_super.contains(r#"<w:vertAlign w:val="superscript"/>"#),
            "packaged Superscript must stay Word Font Effects Super: {packed_super}"
        );
        let packed_sub = style_fragment(&styles, "Subscript");
        assert!(
            packed_sub.contains(r#"<w:vertAlign w:val="subscript"/>"#),
            "packaged Subscript must stay Word Font Effects Sub: {packed_sub}"
        );
        assert!(
            numbering.contains(&lvl0)
                && !numbering.contains(r#"w:left="720" w:hanging="360""#),
            "packaged numbering.xml must keep letter 1.25rem list indent: {numbering}"
        );
        assert!(
            numbering.contains(TASK_UNCHECKED_GLYPH)
                && numbering.contains(TASK_CHECKED_GLYPH)
                && numbering.contains("Segoe UI Symbol")
                && numbering.contains("MS Gothic"),
            "packaged numbering.xml must keep Word ☐/☒ task checkboxes: {numbering}"
        );
    }

    #[test]
    fn write_list_quote_code_spacing_match_word() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        quote.space_before = 80;
        quote.space_after = 200;
        section.body.push(Block::Paragraph(quote));
        let mut code = Paragraph::from_text("docagent prove examples/letter.md");
        code.code_block = true;
        code.space_before = 80;
        code.space_after = 200;
        section.body.push(Block::Paragraph(code));
        section.body.push(Block::Paragraph(numbered_item(
            "Receiving dock is clear",
            0,
            NumberFormat::Bullet,
        )));
        section.body.push(Block::Paragraph(numbered_item(
            "Bay 4, H2O seals intact",
            1,
            NumberFormat::Bullet,
        )));
        section.body.push(Block::Paragraph(numbered_item(
            "Replay the convert instead of hoping",
            0,
            NumberFormat::Bullet,
        )));
        let mut after_list = Paragraph::from_text("Agent replay:");
        after_list.space_after = 200;
        section.body.push(Block::Paragraph(after_list));
        doc.sections.push(section);

        let xml = document_xml(&doc);
        let quote_p = para_containing(&xml, "Replay is evidence");
        let code_p = para_containing(&xml, "docagent prove");
        let dock_p = para_containing(&xml, "Receiving dock is clear");
        let bay_p = para_containing(&xml, "Bay 4, H2O seals intact");
        let last_p = para_containing(&xml, "Replay the convert instead of hoping");
        let quote_before = spacing_attr(&quote_p, "w:before").unwrap_or(0);
        let quote_after = spacing_attr(&quote_p, "w:after").unwrap_or(0);
        let code_before = spacing_attr(&code_p, "w:before").unwrap_or(0);
        let code_after = spacing_attr(&code_p, "w:after").unwrap_or(0);
        assert!(
            (120..=240).contains(&quote_before),
            "quote before must not jam into the previous block: {quote_p}"
        );
        assert!(
            (160..=240).contains(&quote_after),
            "quote after must be Word 8pt floor, not IR 40 twips: {quote_p}"
        );
        assert!(
            ind_attr(&quote_p, "w:left").unwrap_or(0) >= QUOTE_LEFT_TWIPS,
            "quote indent must be Word 0.5in: {quote_p}"
        );
        let quote_bar_sz = hu_to_border_sz(QUOTE_BAR_HU);
        assert!(
            quote_p.contains("<w:pBdr>")
                && quote_p.contains("<w:left ")
                && quote_p.contains(&format!(r#"w:sz="{quote_bar_sz}""#))
                && quote_p.contains(&format!(r#"w:color="{QUOTE_COLOR}""#))
                && p_bdr_left_sz(&quote_p).unwrap_or(0) >= 24,
            "quote must keep letter.html 4px #134e4a left pBdr: {quote_p}"
        );
        assert!(
            quote_p.contains(&format!(r#"w:val="{QUOTE_COLOR}""#))
                && !quote_p.contains(r#"w:val="nil""#)
                && !quote_p.contains(r#"w:color="auto""#),
            "quote bar/text must stay letter teal, not Word auto: {quote_p}"
        );
        assert_eq!(
            code_before,
            hu_to_twips(CODE_BEFORE_HU),
            "code_block before must be letter.css 0.6rem (144 twips), not Word 6pt: {code_p}"
        );
        assert_eq!(
            code_after,
            hu_to_twips(CODE_AFTER_HU),
            "code_block after must be letter.css 1rem (240 twips), not Word 8pt: {code_p}"
        );
        let pad = hu_to_twips(CODE_PAD_X_HU);
        assert!(
            pad >= 240
                && ind_attr(&code_p, "w:left") == Some(pad)
                && ind_attr(&code_p, "w:right") == Some(pad),
            "code_block must keep letter pre 1rem pad, not jammed on the wash: {code_p}"
        );
        assert!(
            xml.contains(r#"w:val="ListParagraph""#),
            "list items must use Word ListParagraph: {xml}"
        );
        assert_eq!(
            (ind_attr(&dock_p, "w:left"), ind_attr(&dock_p, "w:hanging")),
            (Some(LIST_LEFT_TWIPS), Some(LIST_HANGING_TWIPS)),
            "level-0 list indent must be letter 1.25rem/1rem hanging, not Word 0.5in: {dock_p}"
        );
        assert_eq!(
            (ind_attr(&bay_p, "w:left"), ind_attr(&bay_p, "w:hanging")),
            (Some(LIST_LEFT_TWIPS * 2), Some(LIST_HANGING_TWIPS)),
            "level-1 list indent must be nested 2.5rem hanging, not Word 1.0in: {bay_p}"
        );
        let item_after = spacing_attr(&dock_p, "w:after").unwrap_or(0);
        let nested_after = spacing_attr(&bay_p, "w:after").unwrap_or(0);
        let last_after = spacing_attr(&last_p, "w:after").unwrap_or(0);
        let li_after = hu_to_twips(LIST_ITEM_AFTER_HU);
        let nested_gap = hu_to_twips(LIST_NESTED_GAP_HU);
        assert_eq!(
            item_after, nested_gap,
            "parent of nested Bay 4 must keep letter 0.25rem (60 twips), not sibling 0.35rem: {dock_p}"
        );
        assert_eq!(
            nested_after, li_after,
            "nested list items must keep letter 0.35rem, not jammed 0 or body 8pt: {bay_p}"
        );
        let list_after = hu_to_twips(LIST_AFTER_HU);
        assert_eq!(
            last_after, list_after,
            "last list item must keep letter 1rem (240 twips), not Word 8pt: {last_p}"
        );
        assert!(
            last_after > li_after && last_after > 160,
            "Word Normal 8pt / consecutive 0.35rem jams ul,ol vs WeasyPrint 1rem: {last_p}"
        );
    }

    #[test]
    fn write_list_block_after_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        section.body.push(Block::Paragraph(numbered_item(
            "Receiving dock is clear",
            0,
            NumberFormat::Bullet,
        )));
        section.body.push(Block::Paragraph(numbered_item(
            "Bay 4, H2O seals intact",
            1,
            NumberFormat::Bullet,
        )));
        section.body.push(Block::Paragraph(numbered_item(
            "Replay the convert instead of hoping",
            0,
            NumberFormat::Bullet,
        )));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("Agent replay:")));
        section.body.push(Block::Paragraph(numbered_item(
            "Hash the input",
            0,
            NumberFormat::Decimal,
        )));
        section.body.push(Block::Paragraph(numbered_item(
            "Compare the capsule",
            0,
            NumberFormat::Decimal,
        )));
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![image_run(mark_png(), 2400, 2400, "Harbor mark")];
        section.body.push(Block::Paragraph(img_p));
        doc.sections.push(section);

        let list_after = hu_to_twips(LIST_AFTER_HU);
        let li_after = hu_to_twips(LIST_ITEM_AFTER_HU);
        let nested_gap = hu_to_twips(LIST_NESTED_GAP_HU);
        assert_eq!(list_after, 240, "letter ul,ol 1rem is 240 twips");
        assert_eq!(li_after, 84, "letter li 0.35rem is 84 twips");
        assert_eq!(nested_gap, 60, "letter ul ul 0.25rem is 60 twips");
        assert!(
            LIST_AFTER_HU == 1200 && LIST_ITEM_AFTER_HU < LIST_AFTER_HU,
            "last-item 1rem must stay larger than consecutive 0.35rem"
        );
        assert!(
            LIST_NESTED_GAP_HU == 300 && LIST_NESTED_GAP_HU < LIST_ITEM_AFTER_HU,
            "nested 0.25rem must stay tighter than consecutive 0.35rem"
        );

        let xml = document_xml(&doc);
        let dock_p = para_containing(&xml, "Receiving dock is clear");
        let bay_p = para_containing(&xml, "Bay 4, H2O seals intact");
        let last_bullet = para_containing(&xml, "Replay the convert instead of hoping");
        let hash_p = para_containing(&xml, "Hash the input");
        let last_num = para_containing(&xml, "Compare the capsule");
        let body_p = para_containing(&xml, "Agent replay:");
        assert_eq!(
            spacing_attr(&dock_p, "w:after"),
            Some(nested_gap),
            "parent of nested Bay 4 must stay letter 0.25rem, not consecutive 0.35rem: {dock_p}"
        );
        assert_eq!(
            spacing_attr(&bay_p, "w:after"),
            Some(li_after),
            "nested li must stay letter 0.35rem: {bay_p}"
        );
        assert_eq!(
            spacing_attr(&hash_p, "w:after"),
            Some(li_after),
            "consecutive decimal li must stay letter 0.35rem: {hash_p}"
        );
        assert_eq!(
            spacing_attr(&last_bullet, "w:after"),
            Some(list_after),
            "last bullet must keep letter 1rem, not Word 8pt: {last_bullet}"
        );
        assert_eq!(
            spacing_attr(&last_num, "w:after"),
            Some(list_after),
            "last decimal must keep letter 1rem, not Word 8pt: {last_num}"
        );
        assert!(
            spacing_attr(&last_bullet, "w:after").unwrap_or(0) > 160
                && spacing_attr(&last_num, "w:after").unwrap_or(0) > 160
                && !last_bullet.contains(r#"w:after="160""#)
                && !last_num.contains(r#"w:after="160""#),
            "Word Normal 8pt last-item is jammed vs WeasyPrint ul,ol 1rem: {last_bullet} {last_num}"
        );
        assert_eq!(
            spacing_attr(&body_p, "w:after"),
            Some(hu_to_twips(BODY_AFTER_HU)),
            "list 1rem after must not leak onto the next body: {body_p}"
        );
        let list_style = style_fragment(STYLES_XML, "ListParagraph");
        assert_eq!(
            spacing_attr(&list_style, "w:after"),
            Some(li_after),
            "ListParagraph style after must stay consecutive 0.35rem: {list_style}"
        );
        assert!(
            !list_style.contains("<w:contextualSpacing"),
            "Word contextualSpacing jams consecutive li vs WeasyPrint 0.35rem: {list_style}"
        );
        assert_eq!(
            (ind_attr(&dock_p, "w:left"), ind_attr(&dock_p, "w:hanging")),
            (Some(LIST_LEFT_TWIPS), Some(LIST_HANGING_TWIPS)),
            "list block after must keep 1.25rem/1rem hanging: {dock_p}"
        );
        let quote_p = para_containing(&xml, "Replay is evidence");
        assert!(
            ind_attr(&quote_p, "w:left").unwrap_or(0) >= QUOTE_LEFT_TWIPS
                && quote_p.contains("<w:pBdr>")
                && p_bdr_left_sz(&quote_p).unwrap_or(0) >= 24,
            "list block after must keep Quote 0.5in + 4px bar: {quote_p}"
        );
        let hop_tc = cell_containing(&xml, "Hop");
        assert!(
            hop_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && hop_tc.contains(&format!(r#"<w:vAlign w:val="{CELL_VALIGN}"/>"#)),
            "list block after must keep th fill + valign top: {hop_tc}"
        );
        let min_emu = hu_to_emu(MIN_IMAGE_EDGE_HU);
        assert!(
            xml.contains("<a:blip r:embed=")
                && xml.contains(&format!(r#"<wp:extent cx="{min_emu}" cy="{min_emu}"/>"#)),
            "list block after must keep 24pt a:blip: {xml}"
        );
        let mark_p = para_containing(&xml, "<a:blip r:embed=");
        assert_eq!(
            spacing_attr(&mark_p, "w:before"),
            Some(hu_to_twips(IMAGE_BEFORE_HU)),
            "list block after must keep Harbor mark 0.6rem before: {mark_p}"
        );
        assert_eq!(
            spacing_attr(&mark_p, "w:after"),
            Some(hu_to_twips(IMAGE_AFTER_HU)),
            "list block after must keep Harbor mark 0.7rem: {mark_p}"
        );
    }

    #[test]
    fn write_nested_list_gap_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Paragraph(numbered_item(
            "Receiving dock is clear",
            0,
            NumberFormat::Task,
        )));
        section.body.push(Block::Paragraph(numbered_item(
            "Bay 4, H2O seals intact",
            1,
            NumberFormat::Bullet,
        )));
        section.body.push(Block::Paragraph(numbered_item(
            "Capsule hashes travel with the files",
            0,
            NumberFormat::Task,
        )));
        section.body.push(Block::Paragraph(numbered_item(
            "Replay the convert instead of hoping",
            0,
            NumberFormat::Task,
        )));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("Agent replay:")));
        section.body.push(Block::Paragraph(numbered_item(
            "Hash the input",
            0,
            NumberFormat::Decimal,
        )));
        section.body.push(Block::Paragraph(numbered_item(
            "Run Convert",
            0,
            NumberFormat::Decimal,
        )));
        section.body.push(Block::Paragraph(numbered_item(
            "PDF/A",
            1,
            NumberFormat::Bullet,
        )));
        section.body.push(Block::Paragraph(numbered_item(
            "HTML",
            1,
            NumberFormat::Bullet,
        )));
        section.body.push(Block::Paragraph(numbered_item(
            "Compare the capsule",
            0,
            NumberFormat::Decimal,
        )));
        doc.sections.push(section);

        let nested_gap = hu_to_twips(LIST_NESTED_GAP_HU);
        let li_after = hu_to_twips(LIST_ITEM_AFTER_HU);
        let list_after = hu_to_twips(LIST_AFTER_HU);
        assert_eq!(LIST_NESTED_GAP_HU, 300);
        assert_eq!(nested_gap, 60, "letter ul ul 0.25rem is 60 twips");
        assert!(
            LIST_NESTED_GAP_HU < LIST_ITEM_AFTER_HU && nested_gap < li_after,
            "nested 0.25rem must stay tighter than consecutive 0.35rem"
        );

        let xml = document_xml(&doc);
        let dock_p = para_containing(&xml, "Receiving dock is clear");
        let bay_p = para_containing(&xml, "Bay 4, H2O seals intact");
        let capsule_p = para_containing(&xml, "Capsule hashes travel with the files");
        let last_task = para_containing(&xml, "Replay the convert instead of hoping");
        let hash_p = para_containing(&xml, "Hash the input");
        let convert_p = para_containing(&xml, "Run Convert");
        let pdf_p = para_containing(&xml, "PDF/A");
        let html_p = para_containing(&xml, "HTML");
        let last_num = para_containing(&xml, "Compare the capsule");
        assert_eq!(
            spacing_attr(&dock_p, "w:after"),
            Some(nested_gap),
            "dock parent of Bay 4 must keep letter 0.25rem, not sibling 0.35rem: {dock_p}"
        );
        assert_eq!(
            spacing_attr(&convert_p, "w:after"),
            Some(nested_gap),
            "Convert parent of nested PDF/HTML must keep letter 0.25rem: {convert_p}"
        );
        assert_eq!(
            spacing_attr(&bay_p, "w:after"),
            Some(li_after),
            "nested Bay 4 item must stay consecutive 0.35rem: {bay_p}"
        );
        assert_eq!(
            spacing_attr(&capsule_p, "w:after"),
            Some(li_after),
            "consecutive Capsule task must stay 0.35rem: {capsule_p}"
        );
        assert_eq!(
            spacing_attr(&hash_p, "w:after"),
            Some(li_after),
            "consecutive Hash decimal must stay 0.35rem: {hash_p}"
        );
        assert_eq!(
            spacing_attr(&pdf_p, "w:after"),
            Some(li_after),
            "nested PDF sibling must stay 0.35rem: {pdf_p}"
        );
        assert_eq!(
            spacing_attr(&html_p, "w:after"),
            Some(li_after),
            "last nested HTML must stay 0.35rem when the next item is outer: {html_p}"
        );
        assert_eq!(
            spacing_attr(&last_task, "w:after"),
            Some(list_after),
            "last task must keep letter 1rem: {last_task}"
        );
        assert_eq!(
            spacing_attr(&last_num, "w:after"),
            Some(list_after),
            "last decimal must keep letter 1rem: {last_num}"
        );
        assert!(
            !dock_p.contains(r#"w:after="84""#)
                && !dock_p.contains(r#"w:after="240""#)
                && !convert_p.contains(r#"w:after="84""#),
            "python-docx 0.35rem / last-item 1rem parent-of-nested is sparse vs WeasyPrint 0.25rem: {dock_p} {convert_p}"
        );
        let list_style = style_fragment(STYLES_XML, "ListParagraph");
        assert_eq!(
            spacing_attr(&list_style, "w:after"),
            Some(li_after),
            "ListParagraph style after must stay consecutive 0.35rem: {list_style}"
        );
        assert_eq!(
            (ind_attr(&dock_p, "w:left"), ind_attr(&dock_p, "w:hanging")),
            (Some(LIST_LEFT_TWIPS), Some(LIST_HANGING_TWIPS)),
            "nested gap must keep 1.25rem/1rem hanging: {dock_p}"
        );
        assert_eq!(
            (ind_attr(&bay_p, "w:left"), ind_attr(&bay_p, "w:hanging")),
            (Some(LIST_LEFT_TWIPS * 2), Some(LIST_HANGING_TWIPS)),
            "nested gap must keep nested 2.5rem hanging: {bay_p}"
        );
    }

    #[test]
    fn write_after_mark_gap_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h2 = Paragraph::from_text("Delivery confirmation");
        h2.outline_level = Some(2);
        section.body.push(Block::Paragraph(h2));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![image_run(mark_png(), 1200, 1200, "Harbor mark")];
        img_p.space_after = 200;
        section.body.push(Block::Paragraph(img_p));
        let mut body = Paragraph::from_text("6 September 2026");
        body.space_after = 200;
        section.body.push(Block::Paragraph(body));
        section.body.push(Block::Paragraph(numbered_item(
            "Receiving dock is clear",
            0,
            NumberFormat::Task,
        )));
        section.body.push(Block::Paragraph(numbered_item(
            "Bay 4, H2O seals intact",
            1,
            NumberFormat::Bullet,
        )));
        section.body.push(Block::Paragraph(numbered_item(
            "Replay the convert instead of hoping",
            0,
            NumberFormat::Task,
        )));
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        doc.sections.push(section);

        let before = hu_to_twips(IMAGE_BEFORE_HU);
        let after = hu_to_twips(IMAGE_AFTER_HU);
        let body_after = hu_to_twips(BODY_AFTER_HU);
        let nested_gap = hu_to_twips(LIST_NESTED_GAP_HU);
        let list_after = hu_to_twips(LIST_AFTER_HU);
        assert_eq!(IMAGE_BEFORE_HU, 720);
        assert_eq!(IMAGE_AFTER_HU, 840);
        assert_eq!(before, 144, "letter img margin-top 0.6rem is 144 twips");
        assert_eq!(after, 168, "letter img/p collapse 0.7rem is 168 twips");
        assert!(
            IMAGE_BEFORE_HU < IMAGE_AFTER_HU && before < after,
            "img 0.6rem before must stay tighter than collapsed 0.7rem after"
        );
        assert_eq!(BODY_AFTER_HU, 840);
        assert_eq!(body_after, 168, "letter CSS p 0.7rem is 168 twips");
        assert_eq!(
            IMAGE_AFTER_HU, BODY_AFTER_HU,
            "after-mark 0.7rem must match body p 0.7rem"
        );
        assert!(
            after < list_after,
            "after-mark 0.7rem must stay tighter than last-list 1rem"
        );

        let xml = document_xml(&doc);
        let mark_p = para_containing(&xml, "<a:blip r:embed=");
        let body_p = para_containing(&xml, "6 September 2026");
        assert_eq!(
            spacing_attr(&mark_p, "w:before"),
            Some(before),
            "Harbor mark must keep letter 0.6rem before, not Word 0: {mark_p}"
        );
        assert_eq!(
            spacing_attr(&mark_p, "w:after"),
            Some(after),
            "Harbor mark must keep letter 0.7rem, not IR 200 HU or Word 8pt: {mark_p}"
        );
        assert!(
            !mark_p.contains(r#"w:after="160""#) && !mark_p.contains(r#"w:after="240""#),
            "Word Normal 8pt / last-list 1rem after the mark is jammed or sparse vs WeasyPrint 0.7rem: {mark_p}"
        );
        assert_eq!(
            spacing_attr(&body_p, "w:after"),
            Some(body_after),
            "after-mark 0.7rem must not leak onto the next body: {body_p}"
        );
        assert_eq!(
            spacing_attr(&body_p, "w:line"),
            Some(BODY_LINE),
            "after-mark must keep Word 1.15: {body_p}"
        );
        let dock_p = para_containing(&xml, "Receiving dock is clear");
        let last_task = para_containing(&xml, "Replay the convert instead of hoping");
        assert_eq!(
            spacing_attr(&dock_p, "w:after"),
            Some(nested_gap),
            "after-mark must keep nested 0.25rem: {dock_p}"
        );
        assert_eq!(
            spacing_attr(&last_task, "w:after"),
            Some(list_after),
            "after-mark must keep last-list 1rem: {last_task}"
        );
        let hop_tc = cell_containing(&xml, "Hop");
        assert!(
            hop_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && hop_tc.contains(&format!(r#"<w:vAlign w:val="{CELL_VALIGN}"/>"#)),
            "after-mark must keep th fill + valign top: {hop_tc}"
        );
        let h2_p = para_containing(&xml, r#"w:val="Heading2""#);
        assert_eq!(
            spacing_attr(&h2_p, "w:line"),
            Some(HEADING_LINE),
            "after-mark must keep heading 1.2: {h2_p}"
        );
        let emu = hu_to_emu(MIN_IMAGE_EDGE_HU);
        assert!(
            xml.contains(&format!(r#"<wp:extent cx="{emu}" cy="{emu}"/>"#)),
            "after-mark must keep the 24pt a:blip floor: {xml}"
        );
    }

    #[test]
    fn write_before_mark_gap_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h2 = Paragraph::from_text("Delivery confirmation");
        h2.outline_level = Some(2);
        section.body.push(Block::Paragraph(h2));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![image_run(mark_png(), 1200, 1200, "Harbor mark")];
        img_p.space_before = 80;
        img_p.space_after = 200;
        section.body.push(Block::Paragraph(img_p));
        let mut body = Paragraph::from_text("6 September 2026");
        body.space_after = 200;
        section.body.push(Block::Paragraph(body));
        section.body.push(Block::Paragraph(numbered_item(
            "Receiving dock is clear",
            0,
            NumberFormat::Task,
        )));
        section.body.push(Block::Paragraph(numbered_item(
            "Bay 4, H2O seals intact",
            1,
            NumberFormat::Bullet,
        )));
        section.body.push(Block::Paragraph(numbered_item(
            "Replay the convert instead of hoping",
            0,
            NumberFormat::Task,
        )));
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        doc.sections.push(section);

        let before = hu_to_twips(IMAGE_BEFORE_HU);
        let after = hu_to_twips(IMAGE_AFTER_HU);
        let body_after = hu_to_twips(BODY_AFTER_HU);
        let nested_gap = hu_to_twips(LIST_NESTED_GAP_HU);
        let list_after = hu_to_twips(LIST_AFTER_HU);
        assert_eq!(IMAGE_BEFORE_HU, 720);
        assert_eq!(before, 144, "letter img margin-top 0.6rem is 144 twips");
        assert_eq!(IMAGE_AFTER_HU, 840);
        assert_eq!(after, 168, "letter img/p collapse 0.7rem is 168 twips");
        assert_eq!(BODY_AFTER_HU, 840);
        assert_eq!(body_after, 168, "letter CSS p 0.7rem is 168 twips");
        assert_eq!(IMAGE_AFTER_HU, BODY_AFTER_HU);
        assert!(
            IMAGE_BEFORE_HU < IMAGE_AFTER_HU && before < after,
            "before-mark 0.6rem must stay tighter than after-mark 0.7rem"
        );

        let xml = document_xml(&doc);
        let mark_p = para_containing(&xml, "<a:blip r:embed=");
        let body_p = para_containing(&xml, "6 September 2026");
        let h2_p = para_containing(&xml, r#"w:val="Heading2""#);
        assert_eq!(
            spacing_attr(&mark_p, "w:before"),
            Some(before),
            "Harbor mark must keep letter 0.6rem before, not IR 80 HU or Word 0: {mark_p}"
        );
        assert!(
            !mark_p.contains(r#"w:before="0""#)
                && !mark_p.contains(r#"w:before="16""#)
                && !mark_p.contains(r#"w:before="80""#)
                && !mark_p.contains(r#"w:before="160""#)
                && !mark_p.contains(r#"w:before="168""#),
            "Word 0 / IR 80 / H2 4pt / Normal 8pt / after-mark 0.7rem before the mark is jammed or sparse vs WeasyPrint 0.6rem: {mark_p}"
        );
        assert_eq!(
            spacing_attr(&mark_p, "w:after"),
            Some(after),
            "before-mark 0.6rem must keep Harbor after 0.7rem: {mark_p}"
        );
        assert_eq!(
            spacing_attr(&body_p, "w:after"),
            Some(body_after),
            "before-mark 0.6rem must not leak onto the next body: {body_p}"
        );
        assert_eq!(
            spacing_attr(&body_p, "w:line"),
            Some(BODY_LINE),
            "before-mark must keep Word 1.15: {body_p}"
        );
        assert_eq!(
            spacing_attr(&h2_p, "w:line"),
            Some(HEADING_LINE),
            "before-mark must keep heading 1.2: {h2_p}"
        );
        assert!(
            spacing_attr(&h2_p, "w:before").unwrap_or(0) > 0,
            "before-mark 0.6rem must not steal Heading2 before: {h2_p}"
        );
        let dock_p = para_containing(&xml, "Receiving dock is clear");
        let last_task = para_containing(&xml, "Replay the convert instead of hoping");
        assert_eq!(
            spacing_attr(&dock_p, "w:after"),
            Some(nested_gap),
            "before-mark must keep nested 0.25rem: {dock_p}"
        );
        assert_eq!(
            spacing_attr(&last_task, "w:after"),
            Some(list_after),
            "before-mark must keep last-list 1rem: {last_task}"
        );
        let hop_tc = cell_containing(&xml, "Hop");
        assert!(
            hop_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && hop_tc.contains(&format!(r#"<w:vAlign w:val="{CELL_VALIGN}"/>"#)),
            "before-mark must keep th fill + valign top: {hop_tc}"
        );
        let emu = hu_to_emu(MIN_IMAGE_EDGE_HU);
        assert!(
            xml.contains(&format!(r#"<wp:extent cx="{emu}" cy="{emu}"/>"#)),
            "before-mark must keep the 24pt a:blip floor: {xml}"
        );
    }

    #[test]
    fn write_quote_bar_like_word() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        quote.space_before = 80;
        quote.space_after = 200;
        section.body.push(Block::Paragraph(quote));
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        let mut task = Paragraph::from_text("Receiving dock is clear");
        task.numbering = Some(NumberingRef {
            definition_id: 2,
            level: 0,
            start: Some(1),
            format: Some(NumberFormat::Task),
        });
        section.body.push(Block::Paragraph(task));
        let mut flow = Paragraph::from_text("the PDF/");
        let mut sup = Run::text("A");
        sup.style.superscript = true;
        flow.runs.push(sup);
        flow.runs.push(Run::text(" H"));
        let mut sub = Run::text("2");
        sub.style.subscript = true;
        flow.runs.push(sub);
        flow.runs.push(Run::text("O"));
        section.body.push(Block::Paragraph(flow));
        let png = mark_png();
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![image_run(png, 1200, 1200, "Harbor mark")];
        section.body.push(Block::Paragraph(img_p));
        doc.sections.push(section);

        let xml = document_xml(&doc);
        let quote_p = para_containing(&xml, "Replay is evidence");
        let bar_sz = hu_to_border_sz(QUOTE_BAR_HU);
        assert!(
            bar_sz >= 24 && QUOTE_BAR_HU == 300,
            "letter 4px is OOXML sz 24, got {bar_sz}"
        );
        assert!(
            quote_p.contains(r#"w:val="Quote""#)
                && quote_p.contains("<w:pBdr>")
                && quote_p.contains("<w:left ")
                && quote_p.contains(&format!(r#"w:sz="{bar_sz}""#))
                && quote_p.contains(&format!(r#"w:space="{QUOTE_BDR_SPACE_PT}""#))
                && quote_p.contains(&format!(r#"w:color="{QUOTE_COLOR}""#)),
            "blockquote must emit Word left pBdr matching letter.html 4px #134e4a: {quote_p}"
        );
        assert!(
            ind_attr(&quote_p, "w:left").unwrap_or(0) >= QUOTE_LEFT_TWIPS,
            "quote indent must stay Word 0.5in, not a missing bar-only stub: {quote_p}"
        );
        assert_eq!(
            p_bdr_left_sz(&quote_p),
            Some(bar_sz),
            "quote bar must be letter 4px (sz 24 eighths), not Word hairline 6: {quote_p}"
        );
        assert!(
            quote_p.contains(&format!(r#"w:val="{QUOTE_COLOR}""#))
                && !quote_p.contains(r#"w:val="nil""#)
                && !quote_p.contains(r#"w:color="auto""#)
                && !quote_p.contains(r#"w:color="CCC""#)
                && !quote_p.contains(r#"w:color="999999""#)
                && !quote_p.contains(r#"w:sz="6""#)
                && !quote_p.contains(r#"w:sz="8""#),
            "UA/reset/hairline/gray quote bar is not WeasyPrint letter input: {quote_p}"
        );
        assert!(
            quote_p.contains(&format!(r#"<w:color w:val="{QUOTE_COLOR}"/>"#)),
            "quote text must stay letter.html #134e4a like the 4px bar: {quote_p}"
        );

        let quote_style = style_fragment(STYLES_XML, "Quote");
        assert!(
            ind_attr(&quote_style, "w:left").unwrap_or(0) >= QUOTE_LEFT_TWIPS,
            "Quote style indent must stay Word 0.5in: {quote_style}"
        );
        assert!(
            quote_style.contains("<w:pBdr>")
                && quote_style.contains("<w:left ")
                && quote_style.contains(r#"w:sz="24""#)
                && quote_style.contains(&format!(r#"w:color="{QUOTE_COLOR}""#))
                && quote_style.contains(&format!(r#"<w:color w:val="{QUOTE_COLOR}"/>"#))
                && p_bdr_left_sz(&quote_style).unwrap_or(0) >= 24,
            "Quote style must keep letter 4px #134e4a left bar + teal type: {quote_style}"
        );
        assert!(
            !quote_style.contains(r#"w:val="nil""#)
                && !quote_style.contains(r#"w:color="auto""#)
                && !quote_style.contains(r#"w:sz="6""#),
            "Quote style must not reset to Word auto/hairline: {quote_style}"
        );

        let a_run = run_containing(&xml, ">A</w:t>");
        let two_run = run_containing(&xml, ">2</w:t>");
        assert!(
            a_run.contains(r#"<w:vertAlign w:val="superscript"/>"#)
                && two_run.contains(r#"<w:vertAlign w:val="subscript"/>"#),
            "quote bar must keep super/sub vertAlign: {a_run} {two_run}"
        );
        let hop_tc = cell_containing(&xml, "Hop");
        assert!(
            hop_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && hop_tc.contains(&format!(r#"w:color="{TH_RULE_COLOR}""#))
                && hu_to_border_sz(TH_RULE_HU) >= 12,
            "quote bar must keep th teal fill + 2px rule: {hop_tc}"
        );
        let dock_p = para_containing(&xml, "Receiving dock is clear");
        assert_eq!(
            num_id_attr(&dock_p),
            Some(TASK_CHECKED_NUM_ID),
            "quote bar must keep checked task numId 4: {dock_p}"
        );
        assert!(
            xml.contains("<a:blip r:embed="),
            "quote bar must keep 24pt a:blip: {xml}"
        );
        let emu = hu_to_emu(MIN_IMAGE_EDGE_HU);
        assert!(
            xml.contains(&format!(r#"<wp:extent cx="{emu}" cy="{emu}"/>"#)),
            "quote bar must keep the 24pt a:blip floor: {xml}"
        );

        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("quote para");
        };
        assert!(p.quote && p.plain_text().contains("Replay is evidence"));
    }

    #[test]
    fn write_inline_code_and_mark_like_word() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("one convert, ");
        let mut mark = Run::text("three hashes");
        mark.style.highlight = Some(docagent_model::Color::MARK);
        p.runs.push(mark);
        p.runs.push(Run::text(", "));
        let mut bold = Run::text("GPU-free");
        bold.style.bold = true;
        p.runs.push(bold);
        p.runs.push(Run::text(". Run "));
        let mut code = Run::text("prove");
        code.style.code = true;
        p.runs.push(code);
        p.runs.push(Run::text(" twice."));
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = document_xml(&doc);

        let flow = para_containing(&xml, "three hashes");
        assert!(
            flow.contains(">prove</w:t>") && flow.contains(r#"w:val="Code""#),
            "inline code must emit Code, not plain text: {flow}"
        );
        assert!(
            flow.contains("Consolas")
                && flow.contains(&format!(r#"w:val="{CODE_COLOR}""#))
                && flow.contains(&format!(r#"w:fill="{CODE_FILL}""#)),
            "inline code must be letter teal Consolas chip: {flow}"
        );
        let prove_run = run_containing(&flow, ">prove</w:t>");
        assert_eq!(
            style_sz(&prove_run),
            CODE_SPAN_SZ,
            "inline `prove` must stay Word 10pt (.92em of 11pt), not body 11pt: {prove_run}"
        );
        assert!(
            prove_run.contains(&format!(r#"<w:szCs w:val="{CODE_SPAN_SZ}"/>"#))
                && !prove_run.contains(r#"<w:sz w:val="22"/>"#),
            "inline code must keep direct w:szCs 20, not Normal 22: {prove_run}"
        );
        assert!(
            bdr_space_for_color(&flow, CODE_FILL).unwrap_or(0) >= CODE_BDR_SPACE_PT,
            "inline code pad must be letter .1em/.35em ({CODE_BDR_SPACE_PT}pt): {flow}"
        );
        assert!(
            flow.contains(">three hashes</w:t>")
                && (flow.contains(r#"<w:highlight w:val="yellow"/>"#)
                    || flow.contains(&format!(r#"w:fill="{MARK_FILL}""#))),
            "highlight must be a visible Word wash: {flow}"
        );
        assert!(
            flow.contains(r#"w:val="Mark""#)
                && flow.contains(r#"<w:highlight w:val="yellow"/>"#)
                && flow.contains(&format!(r#"w:fill="{MARK_FILL}""#)),
            "mark must keep Word highlighter + letter amber shading: {flow}"
        );
        let mark_run = run_containing(&flow, ">three hashes</w:t>");
        assert!(
            mark_run.contains(&format!(r#"<w:color w:val="{MARK_COLOR}"/>"#))
                && !mark_run.contains(r#"w:val="000000""#)
                && !mark_run.contains(r#"w:color="auto""#),
            "mark run must stay letter.html #134e4a, not highlighter black: {mark_run}"
        );
        assert!(
            bdr_space_for_color(&flow, MARK_FILL).unwrap_or(0) >= MARK_BDR_SPACE_PT,
            "mark pad must be letter .05em/.2em ({MARK_BDR_SPACE_PT}pt): {flow}"
        );

        let code_style = style_fragment(STYLES_XML, "Code");
        assert!(
            code_style.contains("Consolas")
                && code_style.contains(&format!(r#"w:val="{CODE_COLOR}""#))
                && code_style.contains(&format!(r#"w:fill="{CODE_FILL}""#))
                && style_sz(&code_style) == CODE_SPAN_SZ,
            "Code style must stay letter teal Consolas .92em: {code_style}"
        );
        assert!(
            bdr_space_for_color(&code_style, CODE_FILL).unwrap_or(0) >= CODE_BDR_SPACE_PT,
            "Code style pad must stay letter .1em/.35em: {code_style}"
        );
        let mark_style = style_fragment(STYLES_XML, "Mark");
        assert!(
            mark_style.contains(r#"w:val="yellow""#)
                && mark_style.contains(&format!(r#"w:fill="{MARK_FILL}""#))
                && mark_style.contains(&format!(r#"<w:color w:val="{MARK_COLOR}"/>"#))
                && !mark_style.contains(r#"w:val="000000""#),
            "Mark style must stay letter teal on amber, not highlighter black: {mark_style}"
        );
        assert!(
            bdr_space_for_color(&mark_style, MARK_FILL).unwrap_or(0) >= MARK_BDR_SPACE_PT,
            "Mark style pad must stay letter .05em/.2em: {mark_style}"
        );

        let hr = style_fragment(STYLES_XML, "HorizontalLine");
        assert!(
            hr.contains(r#"w:sz="18""#) && hr.contains(r#"w:color="134E4A""#),
            "inline code/mark must keep hr 3px teal: {hr}"
        );
        let link = style_fragment(STYLES_XML, "Hyperlink");
        assert!(
            link.contains(r#"w:val="0D9488""#) && link.contains("<w:u "),
            "inline code/mark must keep Hyperlink teal underline: {link}"
        );
        let h1 = style_fragment(STYLES_XML, "Heading1");
        let h2 = style_fragment(STYLES_XML, "Heading2");
        assert_eq!(style_sz(&h1), 32, "inline code/mark must keep Heading1 16pt: {h1}");
        assert_eq!(style_sz(&h2), 26, "inline code/mark must keep Heading2 13pt: {h2}");
        assert!(
            STYLES_XML.contains(r#"w:styleId="ListParagraph""#)
                && STYLES_XML.contains(r#"w:after="84""#)
                && !STYLES_XML.contains("<w:contextualSpacing"),
            "inline code/mark must keep ListParagraph 0.35rem after"
        );
        assert!(
            STYLES_XML.contains(r#"w:styleId="Quote""#)
                && STYLES_XML.contains(r#"w:styleId="CodeBlock""#),
            "inline code/mark must keep Quote/CodeBlock"
        );
        let strike_style = style_fragment(STYLES_XML, "Strike");
        assert!(
            strike_style.contains("<w:strike/>"),
            "inline code/mark must keep Strike: {strike_style}"
        );
        let under_style = style_fragment(STYLES_XML, "Underline");
        assert!(
            under_style.contains(r#"<w:u w:val="single"/>"#),
            "inline code/mark must keep Underline: {under_style}"
        );
        assert_eq!(hu_to_emu(MIN_IMAGE_EDGE_HU), 304_800);
    }

    #[test]
    fn write_mark_color_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("one convert, ");
        let mut mark = Run::text("three hashes");
        mark.style.highlight = Some(docagent_model::Color::MARK);
        p.runs.push(mark);
        p.runs.push(Run::text(", "));
        let mut code = Run::text("prove");
        code.style.code = true;
        p.runs.push(code);
        section.body.push(Block::Paragraph(p));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        let mut table = Table::from_cells(vec![vec!["Hop".into()], vec!["Input".into()]]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        section
            .body
            .push(Block::Paragraph(task_item("Receiving dock is clear", true)));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![image_run(mark_png(), 2400, 2400, "Harbor mark")];
        section.body.push(Block::Paragraph(img_p));
        doc.sections.push(section);

        let xml = document_xml(&doc);
        let mark_run = run_containing(&xml, ">three hashes</w:t>");
        assert_eq!(MARK_COLOR, BODY_COLOR);
        assert!(
            mark_run.contains(&format!(r#"<w:color w:val="{MARK_COLOR}"/>"#))
                && mark_run.contains(r#"w:val="Mark""#)
                && mark_run.contains(&format!(r#"w:fill="{MARK_FILL}""#))
                && !mark_run.contains(r#"w:val="000000""#)
                && !mark_run.contains(r#"w:color="auto""#),
            "mark run must stay letter.html #134e4a on amber, not highlighter black: {mark_run}"
        );
        let mark_style = style_fragment(STYLES_XML, "Mark");
        assert!(
            mark_style.contains(&format!(r#"<w:color w:val="{MARK_COLOR}"/>"#))
                && mark_style.contains(&format!(r#"w:fill="{MARK_FILL}""#))
                && !mark_style.contains(r#"w:val="000000""#),
            "Mark style type must stay letter #134e4a so style-only Mark is not black-on-yellow: {mark_style}"
        );
        let prove_run = run_containing(&xml, ">prove</w:t>");
        assert_eq!(
            style_sz(&prove_run),
            CODE_SPAN_SZ,
            "mark color must keep inline `prove` at Word 10pt: {prove_run}"
        );
        let body_run = run_containing(&xml, ">6 September 2026</w:t>");
        assert!(
            body_run.contains(&format!(r#"<w:color w:val="{BODY_COLOR}"/>"#))
                && !body_run.contains(&format!(r#"w:fill="{MARK_FILL}""#)),
            "mark color must keep body #134e4a and not leak amber onto Normal: {body_run}"
        );
        let quote_p = para_containing(&xml, "Replay is evidence");
        assert!(
            quote_p.contains("<w:pBdr>")
                && p_bdr_left_sz(&quote_p).unwrap_or(0) >= 24
                && quote_p.contains(&format!(r#"w:color="{QUOTE_COLOR}""#)),
            "mark color must keep quote 4px #134e4a left bar: {quote_p}"
        );
        let hop_tc = cell_containing(&xml, "Hop");
        assert!(
            hop_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && hop_tc.contains(&format!(r#"w:color="{TH_RULE_COLOR}""#)),
            "mark color must keep th teal fill: {hop_tc}"
        );
        let dock_p = para_containing(&xml, "Receiving dock is clear");
        assert_eq!(
            num_id_attr(&dock_p),
            Some(TASK_CHECKED_NUM_ID),
            "mark color must keep checked task numId 4: {dock_p}"
        );
        assert!(
            xml.contains("<a:blip r:embed="),
            "mark color must keep 24pt a:blip: {xml}"
        );
    }

    #[test]
    fn write_strike_and_underline_like_word() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut strike_p = Paragraph::from_text("Agents ");
        let mut strike = Run::text("guess");
        strike.style.strike = true;
        strike_p.runs.push(strike);
        strike_p.runs.push(Run::text("."));
        section.body.push(Block::Paragraph(strike_p));
        let mut under_p = Paragraph::from_text("Please confirm before ");
        let mut under = Run::text("09:00 local");
        under.style.underline = Underline::Single;
        under_p.runs.push(under);
        under_p.runs.push(Run::text(":"));
        section.body.push(Block::Paragraph(under_p));
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        let mut code = Paragraph::from_text("prove");
        code.runs[0].style.code = true;
        section.body.push(Block::Paragraph(code));
        let mut mark = Paragraph::from_text("three hashes");
        mark.runs[0].style.highlight = Some(docagent_model::Color::MARK);
        section.body.push(Block::Paragraph(mark));
        let mut task = Paragraph::from_text("Receiving dock is clear");
        task.numbering = Some(NumberingRef {
            definition_id: 2,
            level: 0,
            start: Some(1),
            format: Some(NumberFormat::Task),
        });
        section.body.push(Block::Paragraph(task));
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        section.body.push(Block::Paragraph(h1));
        doc.sections.push(section);
        let xml = document_xml(&doc);

        let guess_p = para_containing(&xml, "guess");
        assert!(
            guess_p.contains(r#"<w:rStyle w:val="Strike"/>"#)
                && guess_p.contains("<w:strike/>")
                && guess_p.contains(">guess</w:t>"),
            "~~guess~~ must emit Strike + w:strike so Word paints the line, not missing: {guess_p}"
        );
        assert!(
            !guess_p.contains(r#"<w:strike w:val="none"/>"#)
                && !guess_p.contains(r#"<w:strike w:val="0"/>"#)
                && !guess_p.contains(r#"<w:strike w:val="false"/>"#),
            "strike must not reset to a missing line: {guess_p}"
        );
        let confirm_p = para_containing(&xml, "09:00 local");
        assert!(
            confirm_p.contains(r#"<w:rStyle w:val="Underline"/>"#)
                && confirm_p.contains(r#"<w:u w:val="single"/>"#)
                && confirm_p.contains(">09:00 local</w:t>"),
            "__09:00 local__ must emit Underline + w:u so Word paints the rule, not missing: {confirm_p}"
        );
        assert!(
            !confirm_p.contains(r#"<w:u w:val="none"/>"#),
            "underline must not reset to a missing rule: {confirm_p}"
        );

        let strike_style = style_fragment(STYLES_XML, "Strike");
        assert!(
            strike_style.contains("<w:strike/>")
                && !strike_style.contains(r#"w:val="none""#)
                && !strike_style.contains(r#"w:val="false""#),
            "Strike style must be Word strikethrough, not a missing line: {strike_style}"
        );
        let under_style = style_fragment(STYLES_XML, "Underline");
        assert!(
            under_style.contains(r#"<w:u w:val="single"/>"#)
                && !under_style.contains(r#"w:val="none""#),
            "Underline style must be Word Single, not a missing rule: {under_style}"
        );

        let hop_tc = cell_containing(&xml, "Hop");
        assert!(
            hop_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && hop_tc.contains(&format!(r#"w:color="{TH_RULE_COLOR}""#))
                && hu_to_border_sz(TH_RULE_HU) >= 12,
            "strike/underline must keep th teal fill + 2px rule: {hop_tc}"
        );
        let prove_p = para_containing(&xml, "prove");
        assert!(
            prove_p.contains("Consolas")
                && prove_p.contains(&format!(r#"w:fill="{CODE_FILL}""#))
                && prove_p.contains(r#"w:val="Code""#),
            "strike/underline must keep Code Consolas chip: {prove_p}"
        );
        let mark_p = para_containing(&xml, "three hashes");
        assert!(
            mark_p.contains(r#"<w:highlight w:val="yellow"/>"#)
                && mark_p.contains(&format!(r#"w:fill="{MARK_FILL}""#))
                && mark_p.contains(r#"w:val="Mark""#),
            "strike/underline must keep Mark highlight: {mark_p}"
        );
        let dock_p = para_containing(&xml, "Receiving dock is clear");
        assert_eq!(
            num_id_attr(&dock_p),
            Some(TASK_CHECKED_NUM_ID),
            "strike/underline must keep checked task numId 4: {dock_p}"
        );
        let h1_p = para_containing(&xml, r#"w:val="Heading1""#);
        assert!(
            !h1_p.contains("<w:sz "),
            "strike/underline must keep Heading1 16pt style: {h1_p}"
        );
        assert_eq!(
            hu_to_emu(MIN_IMAGE_EDGE_HU),
            304_800,
            "strike/underline must keep 24pt a:blip floor"
        );

        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("strike para");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.strike && matches!(&r.content, RunContent::Text(t) if t == "guess")
        }));
        let Block::Paragraph(p) = &back.sections[0].body[1] else {
            panic!("underline para");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.underline == Underline::Single
                && matches!(&r.content, RunContent::Text(t) if t == "09:00 local")
        }));
    }

    #[test]
    fn write_super_and_sub_like_word() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("the PDF/");
        let mut sup = Run::text("A");
        sup.style.superscript = true;
        p.runs.push(sup);
        p.runs.push(Run::text(" bytes match. Wet cargo is H"));
        let mut sub = Run::text("2");
        sub.style.subscript = true;
        p.runs.push(sub);
        p.runs.push(Run::text("O only. Agents "));
        let mut strike = Run::text("guess");
        strike.style.strike = true;
        p.runs.push(strike);
        p.runs.push(Run::text("."));
        section.body.push(Block::Paragraph(p));
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        let mut code = Paragraph::from_text("prove");
        code.runs[0].style.code = true;
        section.body.push(Block::Paragraph(code));
        let mut mark = Paragraph::from_text("three hashes");
        mark.runs[0].style.highlight = Some(docagent_model::Color::MARK);
        section.body.push(Block::Paragraph(mark));
        let mut task = Paragraph::from_text("Receiving dock is clear");
        task.numbering = Some(NumberingRef {
            definition_id: 2,
            level: 0,
            start: Some(1),
            format: Some(NumberFormat::Task),
        });
        section.body.push(Block::Paragraph(task));
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        section.body.push(Block::Paragraph(h1));
        doc.sections.push(section);
        let xml = document_xml(&doc);

        let flow = para_containing(&xml, "PDF/");
        let a_run = run_containing(&flow, ">A</w:t>");
        let two_run = run_containing(&flow, ">2</w:t>");
        assert!(
            a_run.contains(r#"<w:rStyle w:val="Superscript"/>"#)
                && a_run.contains(r#"<w:vertAlign w:val="superscript"/>"#)
                && a_run.contains(">A</w:t>"),
            "PDF/^A^ must emit Superscript + w:vertAlign so Word raises the A, not missing: {a_run}"
        );
        assert!(
            !a_run.contains(r#"w:val="baseline""#) && !a_run.contains(r#"w:val="subscript""#),
            "superscript must not reset to baseline or subscript: {a_run}"
        );
        assert!(
            two_run.contains(r#"<w:rStyle w:val="Subscript"/>"#)
                && two_run.contains(r#"<w:vertAlign w:val="subscript"/>"#)
                && two_run.contains(">2</w:t>"),
            "H~2~O must emit Subscript + w:vertAlign so Word lowers the 2, not missing: {two_run}"
        );
        assert!(
            !two_run.contains(r#"w:val="baseline""#) && !two_run.contains(r#"w:val="superscript""#),
            "subscript must not reset to baseline or superscript: {two_run}"
        );

        let super_style = style_fragment(STYLES_XML, "Superscript");
        assert!(
            super_style.contains(r#"<w:vertAlign w:val="superscript"/>"#)
                && !super_style.contains(r#"w:val="baseline""#)
                && !super_style.contains(r#"w:val="subscript""#),
            "Superscript style must be Word Font Effects Super, not missing: {super_style}"
        );
        let sub_style = style_fragment(STYLES_XML, "Subscript");
        assert!(
            sub_style.contains(r#"<w:vertAlign w:val="subscript"/>"#)
                && !sub_style.contains(r#"w:val="baseline""#)
                && !sub_style.contains(r#"w:val="superscript""#),
            "Subscript style must be Word Font Effects Sub, not missing: {sub_style}"
        );

        let guess_p = para_containing(&xml, "guess");
        assert!(
            guess_p.contains(r#"<w:rStyle w:val="Strike"/>"#)
                && guess_p.contains("<w:strike/>")
                && guess_p.contains(">guess</w:t>"),
            "super/sub must keep Strike + w:strike: {guess_p}"
        );
        let hop_tc = cell_containing(&xml, "Hop");
        assert!(
            hop_tc.contains(&format!(r#"w:fill="{TH_FILL}""#))
                && hop_tc.contains(&format!(r#"w:color="{TH_RULE_COLOR}""#))
                && hu_to_border_sz(TH_RULE_HU) >= 12,
            "super/sub must keep th teal fill + 2px rule: {hop_tc}"
        );
        let prove_p = para_containing(&xml, "prove");
        assert!(
            prove_p.contains("Consolas")
                && prove_p.contains(&format!(r#"w:fill="{CODE_FILL}""#))
                && prove_p.contains(r#"w:val="Code""#),
            "super/sub must keep Code Consolas chip: {prove_p}"
        );
        let mark_p = para_containing(&xml, "three hashes");
        assert!(
            mark_p.contains(r#"<w:highlight w:val="yellow"/>"#)
                && mark_p.contains(&format!(r#"w:fill="{MARK_FILL}""#))
                && mark_p.contains(r#"w:val="Mark""#),
            "super/sub must keep Mark highlight: {mark_p}"
        );
        let dock_p = para_containing(&xml, "Receiving dock is clear");
        assert_eq!(
            num_id_attr(&dock_p),
            Some(TASK_CHECKED_NUM_ID),
            "super/sub must keep checked task numId 4: {dock_p}"
        );
        let h1_p = para_containing(&xml, r#"w:val="Heading1""#);
        assert!(
            !h1_p.contains("<w:sz "),
            "super/sub must keep Heading1 16pt style: {h1_p}"
        );
        assert_eq!(
            hu_to_emu(MIN_IMAGE_EDGE_HU),
            304_800,
            "super/sub must keep 24pt a:blip floor"
        );

        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("super/sub para");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.superscript
                && !r.style.subscript
                && matches!(&r.content, RunContent::Text(t) if t == "A")
        }));
        assert!(p.runs.iter().any(|r| {
            r.style.subscript
                && !r.style.superscript
                && matches!(&r.content, RunContent::Text(t) if t == "2")
        }));
        assert!(p.runs.iter().any(|r| {
            !r.style.superscript
                && !r.style.subscript
                && matches!(&r.content, RunContent::Text(t) if t.contains("PDF/"))
        }));
        assert!(p.runs.iter().any(|r| {
            r.style.strike && matches!(&r.content, RunContent::Text(t) if t == "guess")
        }));
    }

    #[test]
    fn reads_python_docx_rstyle_and_vertalign_super_sub() {
        let document_xml = concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
            r#"<w:body><w:p>"#,
            r#"<w:r><w:t>PDF/</w:t></w:r>"#,
            r#"<w:r><w:rPr><w:rStyle w:val="Superscript"/></w:rPr><w:t>A</w:t></w:r>"#,
            r#"<w:r><w:t> H</w:t></w:r>"#,
            r#"<w:r><w:rPr><w:vertAlign w:val="subscript"/></w:rPr><w:t>2</w:t></w:r>"#,
            r#"<w:r><w:t>O</w:t></w:r>"#,
            r#"</w:p></w:body></w:document>"#,
        );
        let pkg = pack_docx(&[
            ("[Content_Types].xml", MINIMAL_CONTENT_TYPES),
            ("_rels/.rels", MINIMAL_RELS),
            ("word/document.xml", document_xml.as_bytes()),
        ]);
        let back = read(&pkg).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph {:?}", back.sections[0].body);
        };
        assert!(
            p.runs.iter().any(|r| {
                r.style.superscript
                    && !r.style.subscript
                    && matches!(&r.content, RunContent::Text(t) if t == "A")
            }),
            "Word rStyle Superscript must map to a superscript run, not missing: {:?}",
            p.runs
        );
        assert!(
            p.runs.iter().any(|r| {
                r.style.subscript
                    && !r.style.superscript
                    && matches!(&r.content, RunContent::Text(t) if t == "2")
            }),
            "python-docx w:vertAlign subscript must map to a subscript run, not missing: {:?}",
            p.runs
        );
    }

    #[test]
    fn roundtrip_keeps_float_image_bytes() {
        let png = mark_png();
        assert!(
            png.starts_with(&[0x89, b'P', b'N', b'G']),
            "fixture must be PNG"
        );
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Float(Float::Image(ImageData {
            bytes: png.clone(),
            mime: "image/png".into(),
            width: 7200,
            height: 3600,
            alt_text: Some("float mark".into()),
            wrap: WrapMode::Inline,
        })));
        doc.sections.push(section);

        let xml = document_xml(&doc);
        assert!(xml.contains("<w:drawing>"), "{xml}");
        assert!(xml.contains("<a:blip r:embed="), "{xml}");
        assert!(xml.contains(r#"cx="914400""#), "{xml}");
        assert!(xml.contains(r#"cy="457200""#), "{xml}");
        assert!(xml.contains(r#"descr="float mark""#), "{xml}");

        let bytes = write(&doc).unwrap();
        assert!(sniff(&bytes));
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.clone())).unwrap();
        let mut media = Vec::new();
        zip.by_name("word/media/image1.png")
            .unwrap()
            .read_to_end(&mut media)
            .unwrap();
        assert_eq!(media, png);

        let back = read(&bytes).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph {:?}", back.sections[0].body);
        };
        let Some(img) = p.runs.iter().find_map(|r| match &r.content {
            RunContent::Inline(InlineObject::Image(img)) => Some(img),
            _ => None,
        }) else {
            panic!("missing image run: {:?}", p.runs);
        };
        assert_eq!(img.bytes, png);
        assert_eq!(img.mime, "image/png");
        assert_eq!(img.width, 7200);
        assert_eq!(img.height, 3600);
        assert_eq!(img.alt_text.as_deref(), Some("float mark"));
        assert_eq!(img.wrap, WrapMode::Inline);
    }

    #[test]
    fn roundtrip_keeps_jpeg_image_bytes() {
        let jpeg = tiny_jpeg();
        assert!(
            jpeg.starts_with(&[0xFF, 0xD8, 0xFF]),
            "fixture must be JPEG"
        );
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![jpeg_run(jpeg.clone(), 7200, 3600, "JPEG mark")];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);

        let xml = document_xml(&doc);
        assert!(xml.contains("<w:drawing>"), "{xml}");
        assert!(xml.contains("<a:blip r:embed="), "{xml}");
        assert!(xml.contains(r#"cx="914400""#), "{xml}");
        assert!(xml.contains(r#"cy="457200""#), "{xml}");
        assert!(xml.contains(r#"descr="JPEG mark""#), "{xml}");

        let rels = document_rels(&doc);
        assert!(rels.contains("/relationships/image"), "{rels}");
        assert!(rels.contains("media/image1.jpeg"), "{rels}");

        let types = content_types_xml(&collect_images(&doc));
        assert!(types.contains(r#"Extension="jpeg""#), "{types}");
        assert!(types.contains("image/jpeg"), "{types}");

        let bytes = write(&doc).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.clone())).unwrap();
        let mut media = Vec::new();
        zip.by_name("word/media/image1.jpeg")
            .unwrap()
            .read_to_end(&mut media)
            .unwrap();
        assert_eq!(media, jpeg);

        let types = {
            let mut f = zip.by_name("[Content_Types].xml").unwrap();
            let mut s = String::new();
            f.read_to_string(&mut s).unwrap();
            s
        };
        assert!(types.contains(r#"Extension="jpeg""#), "{types}");
        assert!(types.contains("image/jpeg"), "{types}");

        let back = read(&bytes).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph {:?}", back.sections[0].body);
        };
        let Some(img) = p.runs.iter().find_map(|r| match &r.content {
            RunContent::Inline(InlineObject::Image(img)) => Some(img),
            _ => None,
        }) else {
            panic!("missing image run: {:?}", p.runs);
        };
        assert_eq!(img.bytes, jpeg);
        assert_eq!(img.mime, "image/jpeg");
        assert_eq!(img.width, 7200);
        assert_eq!(img.height, 3600);
        assert_eq!(img.alt_text.as_deref(), Some("JPEG mark"));
        assert_eq!(img.wrap, WrapMode::Inline);
    }

    #[test]
    fn roundtrip_keeps_square_wrap() {
        let png = mark_png();
        assert!(
            png.starts_with(&[0x89, b'P', b'N', b'G']),
            "fixture must be PNG"
        );
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![Run {
            style: CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(ImageData {
                bytes: png.clone(),
                mime: "image/png".into(),
                width: 7200,
                height: 3600,
                alt_text: Some("square wrap".into()),
                wrap: WrapMode::Square,
            })),
        }];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);

        let xml = document_xml(&doc);
        assert!(xml.contains("<wp:wrapSquare"), "{xml}");
        assert!(xml.contains("<wp:anchor"), "{xml}");
        assert!(!xml.contains("<wp:inline"), "{xml}");
        assert!(xml.contains(r#"behindDoc="0""#), "{xml}");
        assert!(xml.contains("<a:blip r:embed="), "{xml}");
        assert!(xml.contains(r#"cx="914400""#), "{xml}");
        assert!(xml.contains(r#"cy="457200""#), "{xml}");
        assert!(xml.contains(r#"descr="square wrap""#), "{xml}");

        let bytes = write(&doc).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.clone())).unwrap();
        let mut media = Vec::new();
        zip.by_name("word/media/image1.png")
            .unwrap()
            .read_to_end(&mut media)
            .unwrap();
        assert_eq!(media, png);

        let back = read(&bytes).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph {:?}", back.sections[0].body);
        };
        let Some(img) = p.runs.iter().find_map(|r| match &r.content {
            RunContent::Inline(InlineObject::Image(img)) => Some(img),
            _ => None,
        }) else {
            panic!("missing image run: {:?}", p.runs);
        };
        assert_eq!(img.bytes, png);
        assert_eq!(img.mime, "image/png");
        assert_eq!(img.width, 7200);
        assert_eq!(img.height, 3600);
        assert_eq!(img.alt_text.as_deref(), Some("square wrap"));
        assert_eq!(img.wrap, WrapMode::Square);
    }

    #[test]
    fn roundtrip_keeps_behind_wrap() {
        let png = mark_png();
        assert!(
            png.starts_with(&[0x89, b'P', b'N', b'G']),
            "fixture must be PNG"
        );
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![Run {
            style: CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(ImageData {
                bytes: png.clone(),
                mime: "image/png".into(),
                width: 7200,
                height: 3600,
                alt_text: Some("behind wrap".into()),
                wrap: WrapMode::Behind,
            })),
        }];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);

        let xml = document_xml(&doc);
        assert!(xml.contains("<wp:wrapNone"), "{xml}");
        assert!(xml.contains("<wp:anchor"), "{xml}");
        assert!(!xml.contains("<wp:inline"), "{xml}");
        assert!(xml.contains(r#"behindDoc="1""#), "{xml}");
        assert!(xml.contains("<a:blip r:embed="), "{xml}");
        assert!(xml.contains(r#"cx="914400""#), "{xml}");
        assert!(xml.contains(r#"cy="457200""#), "{xml}");
        assert!(xml.contains(r#"descr="behind wrap""#), "{xml}");

        let bytes = write(&doc).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.clone())).unwrap();
        let mut media = Vec::new();
        zip.by_name("word/media/image1.png")
            .unwrap()
            .read_to_end(&mut media)
            .unwrap();
        assert_eq!(media, png);

        let back = read(&bytes).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph {:?}", back.sections[0].body);
        };
        let Some(img) = p.runs.iter().find_map(|r| match &r.content {
            RunContent::Inline(InlineObject::Image(img)) => Some(img),
            _ => None,
        }) else {
            panic!("missing image run: {:?}", p.runs);
        };
        assert_eq!(img.bytes, png);
        assert_eq!(img.mime, "image/png");
        assert_eq!(img.width, 7200);
        assert_eq!(img.height, 3600);
        assert_eq!(img.alt_text.as_deref(), Some("behind wrap"));
        assert_eq!(img.wrap, WrapMode::Behind);
    }

    #[test]
    fn roundtrip_keeps_in_front_wrap() {
        let png = mark_png();
        assert!(
            png.starts_with(&[0x89, b'P', b'N', b'G']),
            "fixture must be PNG"
        );
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![Run {
            style: CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(ImageData {
                bytes: png.clone(),
                mime: "image/png".into(),
                width: 7200,
                height: 3600,
                alt_text: Some("in front wrap".into()),
                wrap: WrapMode::InFront,
            })),
        }];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);

        let xml = document_xml(&doc);
        assert!(xml.contains("<wp:wrapNone"), "{xml}");
        assert!(xml.contains("<wp:anchor"), "{xml}");
        assert!(!xml.contains("<wp:inline"), "{xml}");
        assert!(!xml.contains(r#"behindDoc="1""#), "{xml}");
        assert!(xml.contains(r#"behindDoc="0""#), "{xml}");
        assert!(xml.contains("<a:blip r:embed="), "{xml}");
        assert!(xml.contains(r#"cx="914400""#), "{xml}");
        assert!(xml.contains(r#"cy="457200""#), "{xml}");
        assert!(xml.contains(r#"descr="in front wrap""#), "{xml}");

        let bytes = write(&doc).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.clone())).unwrap();
        let mut media = Vec::new();
        zip.by_name("word/media/image1.png")
            .unwrap()
            .read_to_end(&mut media)
            .unwrap();
        assert_eq!(media, png);

        let back = read(&bytes).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph {:?}", back.sections[0].body);
        };
        let Some(img) = p.runs.iter().find_map(|r| match &r.content {
            RunContent::Inline(InlineObject::Image(img)) => Some(img),
            _ => None,
        }) else {
            panic!("missing image run: {:?}", p.runs);
        };
        assert_eq!(img.bytes, png);
        assert_eq!(img.mime, "image/png");
        assert_eq!(img.width, 7200);
        assert_eq!(img.height, 3600);
        assert_eq!(img.alt_text.as_deref(), Some("in front wrap"));
        assert_eq!(img.wrap, WrapMode::InFront);
    }

    #[test]
    fn roundtrip_keeps_tight_wrap() {
        let png = mark_png();
        assert!(
            png.starts_with(&[0x89, b'P', b'N', b'G']),
            "fixture must be PNG"
        );
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![Run {
            style: CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(ImageData {
                bytes: png.clone(),
                mime: "image/png".into(),
                width: 7200,
                height: 3600,
                alt_text: Some("tight wrap".into()),
                wrap: WrapMode::Tight,
            })),
        }];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);

        let xml = document_xml(&doc);
        assert!(xml.contains("<wp:wrapTight"), "{xml}");
        assert!(xml.contains("<wp:anchor"), "{xml}");
        assert!(!xml.contains("<wp:inline"), "{xml}");
        assert!(xml.contains(r#"behindDoc="0""#), "{xml}");
        assert!(xml.contains("<a:blip r:embed="), "{xml}");
        assert!(xml.contains(r#"cx="914400""#), "{xml}");
        assert!(xml.contains(r#"cy="457200""#), "{xml}");
        assert!(xml.contains(r#"descr="tight wrap""#), "{xml}");

        let bytes = write(&doc).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.clone())).unwrap();
        let mut media = Vec::new();
        zip.by_name("word/media/image1.png")
            .unwrap()
            .read_to_end(&mut media)
            .unwrap();
        assert_eq!(media, png);

        let back = read(&bytes).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph {:?}", back.sections[0].body);
        };
        let Some(img) = p.runs.iter().find_map(|r| match &r.content {
            RunContent::Inline(InlineObject::Image(img)) => Some(img),
            _ => None,
        }) else {
            panic!("missing image run: {:?}", p.runs);
        };
        assert_eq!(img.bytes, png);
        assert_eq!(img.mime, "image/png");
        assert_eq!(img.width, 7200);
        assert_eq!(img.height, 3600);
        assert_eq!(img.alt_text.as_deref(), Some("tight wrap"));
        assert_eq!(img.wrap, WrapMode::Tight);
    }

    #[test]
    fn reads_python_docx_style_wrap_square() {
        let png = mark_png();
        let types = br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
        let rels = br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/></Relationships>"#;
        let document_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:drawing><wp:anchor distT="0" distB="0" distL="114300" distR="114300" simplePos="0" relativeHeight="251658240" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1"><wp:simplePos x="0" y="0"/><wp:positionH relativeFrom="column"><wp:posOffset>0</wp:posOffset></wp:positionH><wp:positionV relativeFrom="paragraph"><wp:posOffset>0</wp:posOffset></wp:positionV><wp:extent cx="914400" cy="457200"/><wp:effectExtent l="0" t="0" r="0" b="0"/><wp:wrapSquare wrapText="bothSides"/><wp:docPr id="1" name="Picture 1" descr="square wrap"/><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/></wp:cNvGraphicFramePr><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="0" name="image1.png" descr="square wrap"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rId4"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="457200"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r></w:p></w:body></w:document>"#;
        let pkg = pack_docx(&[
            ("[Content_Types].xml", types.as_slice()),
            ("_rels/.rels", MINIMAL_RELS),
            ("word/_rels/document.xml.rels", rels.as_slice()),
            ("word/document.xml", document_xml.as_slice()),
            ("word/media/image1.png", png.as_slice()),
        ]);
        assert!(sniff(&pkg));
        let back = read(&pkg).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        let Some(img) = p.runs.iter().find_map(|r| match &r.content {
            RunContent::Inline(InlineObject::Image(img)) => Some(img),
            _ => None,
        }) else {
            panic!("missing image: {:?}", p.runs);
        };
        assert_eq!(img.bytes, png);
        assert_eq!(img.mime, "image/png");
        assert_eq!(img.width, 7200);
        assert_eq!(img.height, 3600);
        assert_eq!(img.alt_text.as_deref(), Some("square wrap"));
        assert_eq!(img.wrap, WrapMode::Square);
    }

    #[test]
    fn write_sniffs_png_magic_without_mime() {
        let png = mark_png();
        assert!(
            png.starts_with(&[0x89, b'P', b'N', b'G']),
            "fixture must be PNG"
        );
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![Run {
            style: CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(ImageData {
                bytes: png.clone(),
                mime: String::new(),
                width: HU_PER_INCH,
                height: HU_PER_INCH,
                alt_text: Some("sniffed png".into()),
                wrap: WrapMode::Inline,
            })),
        }];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = document_xml(&doc);
        assert!(xml.contains("<a:blip r:embed="), "{xml}");
        let rels = document_rels(&doc);
        assert!(rels.contains("media/image1.png"), "{rels}");
        let types = content_types_xml(&collect_images(&doc));
        assert!(types.contains(r#"Extension="png""#), "{types}");
        assert!(types.contains("image/png"), "{types}");
        let bytes = write(&doc).unwrap();
        assert!(sniff(&bytes));
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.clone())).unwrap();
        let mut media = Vec::new();
        zip.by_name("word/media/image1.png")
            .unwrap()
            .read_to_end(&mut media)
            .unwrap();
        assert_eq!(media, png);
        let back = read(&bytes).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        let Some(img) = p.runs.iter().find_map(|r| match &r.content {
            RunContent::Inline(InlineObject::Image(img)) => Some(img),
            _ => None,
        }) else {
            panic!("missing image: {:?}", p.runs);
        };
        assert_eq!(img.bytes, png);
        assert_eq!(img.mime, "image/png");
    }

    #[test]
    fn write_sniffs_jpeg_magic_without_mime() {
        let jpeg = tiny_jpeg();
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![Run {
            style: CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(ImageData {
                bytes: jpeg.clone(),
                mime: String::new(),
                width: HU_PER_INCH,
                height: HU_PER_INCH,
                alt_text: Some("sniffed".into()),
                wrap: WrapMode::Inline,
            })),
        }];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let rels = document_rels(&doc);
        assert!(rels.contains("media/image1.jpeg"), "{rels}");
        let types = content_types_xml(&collect_images(&doc));
        assert!(types.contains("image/jpeg"), "{types}");
        let bytes = write(&doc).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.clone())).unwrap();
        assert!(zip.by_name("word/media/image1.jpeg").is_ok());
        let back = read(&bytes).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        let Some(img) = p.runs.iter().find_map(|r| match &r.content {
            RunContent::Inline(InlineObject::Image(img)) => Some(img),
            _ => None,
        }) else {
            panic!("missing image: {:?}", p.runs);
        };
        assert_eq!(img.bytes, jpeg);
        assert_eq!(img.mime, "image/jpeg");
    }

    #[test]
    fn reads_python_docx_style_jpeg() {
        let jpeg = tiny_jpeg();
        let types = br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="jpeg" ContentType="image/jpeg"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
        let rels = br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.jpeg"/></Relationships>"#;
        let document_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="914400" cy="457200"/><wp:docPr id="1" name="Picture 1" descr="JPEG mark"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="0" name="image1.jpeg" descr="JPEG mark"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rId4"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="457200"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p></w:body></w:document>"#;
        let pkg = pack_docx(&[
            ("[Content_Types].xml", types.as_slice()),
            ("_rels/.rels", MINIMAL_RELS),
            ("word/_rels/document.xml.rels", rels.as_slice()),
            ("word/document.xml", document_xml.as_slice()),
            ("word/media/image1.jpeg", jpeg.as_slice()),
        ]);
        assert!(sniff(&pkg));
        let back = read(&pkg).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        let Some(img) = p.runs.iter().find_map(|r| match &r.content {
            RunContent::Inline(InlineObject::Image(img)) => Some(img),
            _ => None,
        }) else {
            panic!("missing image: {:?}", p.runs);
        };
        assert_eq!(img.bytes, jpeg);
        assert_eq!(img.mime, "image/jpeg");
        assert_eq!(img.width, 7200);
        assert_eq!(img.height, 3600);
        assert_eq!(img.alt_text.as_deref(), Some("JPEG mark"));
    }

    #[test]
    fn reads_python_docx_style_blip() {
        let png = mark_png();
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut cursor);
            let opt = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            zip.start_file("[Content_Types].xml", opt).unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
            )
            .unwrap();
            zip.start_file("_rels/.rels", opt).unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
            )
            .unwrap();
            zip.start_file("word/_rels/document.xml.rels", opt).unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/></Relationships>"#,
            )
            .unwrap();
            zip.start_file("word/document.xml", opt).unwrap();
            zip.write_all(
                br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:wpc="http://schemas.microsoft.com/office/word/2010/wordprocessingCanvas" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:o="urn:schemas-microsoft-com:office:office" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" xmlns:v="urn:schemas-microsoft-com:vml" xmlns:wp14="http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:w10="urn:schemas-microsoft-com:office:word" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" xmlns:wpg="http://schemas.microsoft.com/office/word/2010/wordprocessingGroup" xmlns:wpi="http://schemas.microsoft.com/office/word/2010/wordprocessingInk" xmlns:wne="http://schemas.microsoft.com/office/word/2006/wordml" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><w:body><w:p><w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="1270000" cy="635000"/><wp:docPr id="1" name="Picture 1" descr="mark"/><a:graphic xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:nvPicPr><pic:cNvPr id="0" name="mark.png"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rId4" cstate="print"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="1270000" cy="635000"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p></w:body></w:document>"#,
            )
            .unwrap();
            zip.start_file("word/media/image1.png", opt).unwrap();
            zip.write_all(&png).unwrap();
            zip.finish().unwrap();
        }
        let pkg = cursor.into_inner();
        assert!(sniff(&pkg));
        let back = read(&pkg).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        let Some(img) = p.runs.iter().find_map(|r| match &r.content {
            RunContent::Inline(InlineObject::Image(img)) => Some(img),
            _ => None,
        }) else {
            panic!("missing image: {:?}", p.runs);
        };
        assert_eq!(img.bytes, png);
        assert_eq!(img.mime, "image/png");
        assert_eq!(img.width, 10_000);
        assert_eq!(img.height, 5_000);
        assert_eq!(img.alt_text.as_deref(), Some("mark"));
    }

    #[test]
    fn roundtrip_keeps_cell_image() {
        let png = mark_png();
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![vec!["plain".into()]]);
        let mut p = Paragraph::from_text("");
        p.runs = vec![image_run(png.clone(), HU_PER_INCH, HU_PER_INCH, "cell mark")];
        table.rows[0].cells[0].blocks = vec![Block::Paragraph(p)];
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let back = roundtrip(&doc).unwrap();
        let Block::Table(t) = &back.sections[0].body[0] else {
            panic!("table");
        };
        let Block::Paragraph(cell) = &t.rows[0].cells[0].blocks[0] else {
            panic!("cell para");
        };
        assert!(cell.runs.iter().any(|r| matches!(
            &r.content,
            RunContent::Inline(InlineObject::Image(img)) if img.bytes == png
        )));
    }

    #[test]
    fn loss_diagnostics_report_lineseg() {
        let mut hwp = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("hinted");
        p.layout_hints.push(LayoutHint {
            text_start: 0,
            x: 0,
            y: 10,
            width: 100,
            line_height: 20,
            text_height: 10,
            baseline: 8,
            spacing: 4,
            flags: 0,
        });
        section.body.push(Block::Paragraph(p));
        hwp.sections.push(section);
        let (_bytes, loss) = hwp_to_docx_with_loss(&hwp).unwrap();
        assert!(loss.iter().any(|d| d.message.contains("LineSeg")));
    }
}
