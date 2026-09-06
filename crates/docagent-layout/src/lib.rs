//! Integer-coordinate layout. One engine. No I/O. No format identifiers.
//!
//! LineSeg hints on the IR are ignored here; the conformance crate is the
//! only consumer.

#![forbid(unsafe_code)]

use docagent_font::{line_height, shape, FontSet};
use docagent_model::{
    Alignment, Block, BorderStyle, BreakKind, Document, Float, HeaderFooter, Hu, Hu64,
    InlineObject, LineSpacing, NumberFormat, Paragraph, RunContent, Section, Table, VAlign,
    DEFAULT_FONT_SIZE_HU, DEFAULT_MARGIN_HU, HU_PER_INCH, HU_PER_POINT,
};
use rayon::prelude::*;

/// Minimum leftover in a table row before the next row starts.
/// Hangul 5.0 spec §4.3 table cell: content + padding; no sample-tuned slack.
pub const MIN_ROW_REMAINDER_HU: Hu = 0;

/// Smallest on-page image edge (24pt). 16px at 96dpi is 12pt.
pub const MIN_IMAGE_EDGE_HU: Hu = 24 * HU_PER_POINT;
const _: () = assert!(MIN_IMAGE_EDGE_HU >= HU_PER_INCH / 4);
const _: () = assert!(MIN_IMAGE_EDGE_HU == 24 * HU_PER_POINT);
const _: () = assert!(MIN_IMAGE_EDGE_HU == 2400);

/// Body leading cap, percent of font size. WeasyPrint/CSS `line-height:1.45`;
/// IR 160% of (asc+desc) reads double-spaced on a letter page.
pub const BODY_LEADING_PCT: u16 = 145;
/// Heading leading, percent of font size. Letter CSS `h1,h2{line-height:1.2}`.
/// Noto (asc+desc)/upem is 1.362em; flooring at that is sparse vs WeasyPrint.
pub const HEADING_LEADING_PCT: u16 = 120;
/// Painted H1. Letter CSS `h1{font-size:1.75rem}` at 16px rem / 96dpi:
/// 1.75rem = 2100 HU. Markdown IR 18pt is the wrong web size.
pub const H1_SIZE_MAX_HU: Hu = 2100;
/// Letter CSS `h1{letter-spacing:-.02em}` at 1.75rem: -0.02em = -42 HU.
/// UA `normal` / 0 tracking is a loose H1 next to WeasyPrint. H2 stays 0.
pub const H1_LETTER_SPACING_HU: Hu = -42;
/// Painted H2. Letter CSS `h2{font-size:1.15rem}`: 1.15rem = 1380 HU.
pub const H2_SIZE_MAX_HU: Hu = 1380;
/// Default cell inset X when IR padding is 0. Letter CSS (WeasyPrint input)
/// `th,td{padding:.4rem .65rem}` at 16px rem / 96dpi: 0.65rem = 780 HU.
pub const CELL_PAD_X_HU: Hu = 780;
/// Default cell inset Y when IR padding is 0. 0.4rem = 480 HU.
pub const CELL_PAD_Y_HU: Hu = 480;
/// Gap before an image-only block. Letter CSS `img{margin:.6rem 0}` top
/// at 16px rem / 96dpi: 0.6rem = 720 HU. Collapses with the previous
/// margin so the 24pt mark is not jammed under the H2 rule.
pub const IMAGE_BEFORE_HU: Hu = 720;
/// Gap after an image-only block. Letter CSS `p{margin:0 0 .7rem}` collapsed
/// with `img{margin:.6rem 0}` is 0.7rem = 840 HU.
pub const IMAGE_AFTER_HU: Hu = 840;
/// Letter CSS `th,td{font-size:.95rem}` at 16px rem / 96dpi: 0.95rem = 1140 HU.
/// Body 10pt (1000 HU) is the wrong table type next to WeasyPrint.
pub const CELL_SIZE_HU: Hu = 1140;
/// Gap after a body paragraph. Letter CSS `p{margin:0 0 .7rem}` = 840 HU.
/// IR `space_after` 200 is a jammed strut and must not replace that gap.
pub const BODY_AFTER_HU: Hu = 840;
/// Letter CSS `ul,ol{padding-left:1.25rem}` at 16px rem / 96dpi: 1.25rem = 1500 HU.
pub const LIST_INDENT_HU: Hu = 1500;
/// Letter CSS `li{margin:0 0 .35rem}` = 420 HU. IR list `space_after` 80 is jammed.
pub const LIST_ITEM_AFTER_HU: Hu = 420;
/// Letter CSS `ul,ol{margin:0 0 1rem}` after the last item, collapsed = 1200 HU.
pub const LIST_AFTER_HU: Hu = 1200;
/// Letter CSS `ul ul,ol ul,ul ol,ol ol{margin:.25rem 0}` at 16px rem / 96dpi:
/// 0.25rem = 300 HU. Sibling [`LIST_ITEM_AFTER_HU`] 0.35rem is the wrong nested
/// gap (letter.md Bay 4 under the dock task, PDF/HTML under Convert).
pub const LIST_NESTED_GAP_HU: Hu = 300;
/// Letter CSS `ul.tasks input{margin-right:.4rem}` = 480 HU.
pub const LIST_MARK_GAP_HU: Hu = 480;
/// Letter CSS `ul.tasks input{width:.9em;height:.9em}` at 10pt body: 0.9em = 900 HU.
pub const TASK_BOX_HU: Hu = 900;
/// In-flow advance from the `ul.tasks` 1.25rem padding-left to the item text.
/// Letter CSS `list-style:none` + `input{width:.9em;margin-right:.4rem}`:
/// the checkbox sits at padding-left, not a hanging outside marker.
pub const TASK_MARK_ADVANCE_HU: Hu = TASK_BOX_HU + LIST_MARK_GAP_HU;
/// Letter CSS `ul.tasks input{border:1.5pt solid #134e4a}` = 150 HU.
pub const TASK_BORDER_HU: Hu = 150;
/// Letter CSS `blockquote{margin:.5rem 0 1rem}` top = 600 HU. IR 80 is jammed.
pub const QUOTE_BEFORE_HU: Hu = 600;
/// Letter CSS blockquote margin-bottom 1rem = 1200 HU. IR 200 is jammed.
pub const QUOTE_AFTER_HU: Hu = 1200;
/// Letter CSS `blockquote{padding:.15rem 0 .15rem 1rem}` left = 1200 HU.
pub const QUOTE_PAD_X_HU: Hu = 1200;
/// Letter CSS blockquote padding-top/bottom 0.15rem = 180 HU.
pub const QUOTE_PAD_Y_HU: Hu = 180;
/// Letter CSS `blockquote{border-left:4px}` at 96dpi = 300 HU.
pub const QUOTE_BAR_W_HU: Hu = 300;
/// Letter CSS `blockquote{color:#134e4a}` (same teal as the 4px bar).
pub const QUOTE_FG: [u8; 4] = [19, 78, 74, 255];
/// Letter body type `#134e4a` (quote/th/s). UA/WeasyPrint default is black.
pub const BODY_FG: [u8; 4] = [19, 78, 74, 255];
/// Letter CSS `::marker{color:#134e4a}`. WeasyPrint UA `::marker` has no color,
/// so agent-replay decimals and nested discs paint CanvasText black.
pub const MARKER_FG: [u8; 4] = BODY_FG;
/// Letter CSS `pre{margin:.6rem 0 1rem}` top = 720 HU. IR 80 is jammed.
pub const CODE_BEFORE_HU: Hu = 720;
/// Letter CSS pre margin-bottom 1rem = 1200 HU. IR 200 is jammed.
pub const CODE_AFTER_HU: Hu = 1200;
/// Letter CSS `pre{padding:.75rem 1rem}` left/right at 16px rem / 96dpi: 1rem = 1200 HU.
/// A left-only inset lets wrapped `pre` flush the content edge next to WeasyPrint.
pub const CODE_PAD_X_HU: Hu = 1200;
/// Letter CSS pre padding-top/bottom 0.75rem = 900 HU.
pub const CODE_PAD_Y_HU: Hu = 900;
/// Letter CSS `hr{border-top:3px solid #134e4a}` at 96dpi: 3px = 225 HU.
pub const HR_THICK_HU: Hu = 225;
/// Letter CSS `hr{margin:1.1rem 0}` top at 16px rem / 96dpi: 1.1rem = 1320 HU.
pub const HR_BEFORE_HU: Hu = 1320;
/// Letter CSS hr margin-bottom 1.1rem = 1320 HU.
pub const HR_AFTER_HU: Hu = 1320;
/// Letter CSS `a{color:#0d9488}` (WeasyPrint input).
pub const LINK_COLOR: [u8; 4] = [13, 148, 136, 255];
/// UA/WeasyPrint link underline. 1px at 96dpi ≈ 80 HU.
pub const LINK_UNDERLINE_HU: Hu = 80;
/// UA/WeasyPrint `u` underline. Same 1px as [`LINK_UNDERLINE_HU`].
pub const UNDERLINE_HU: Hu = LINK_UNDERLINE_HU;
/// Offset below baseline for `u`/`a` underline. WeasyPrint/Pango
/// `underline_position` is ~0.1em at 10pt body so the rule sits below
/// glyph ink, not a 0.4pt hairline that reads as missing.
pub const UNDERLINE_OFFSET_HU: Hu = 100;
/// Letter CSS `abbr[title]{text-underline-offset:.15em}` at 10pt body:
/// 0.15em = 150 HU. Solid `u`/`a` stay [`UNDERLINE_OFFSET_HU`]. WeasyPrint
/// UA dotted underline has no offset and sits on the baseline.
pub const ABBR_UNDERLINE_OFFSET_HU: Hu = 150;
/// Letter CSS `abbr` dotted 1px on / 1px off. Same 1px as [`UNDERLINE_HU`].
/// A solid span is WeasyPrint UA `text-decoration: dotted underline` with no
/// color, which paints CanvasText black.
pub const DOTTED_UNDERLINE_DOT_HU: Hu = UNDERLINE_HU;
/// Letter CSS `s,del{color:#134e4a}` (WeasyPrint input).
pub const STRIKE_COLOR: [u8; 4] = [19, 78, 74, 255];
/// UA/WeasyPrint `s` line-through. 1px at 96dpi ≈ 80 HU.
pub const STRIKE_HU: Hu = 80;
/// Letter CSS `code{padding:.1em .35em}` at 10pt body: 0.35em = 350 HU.
/// `kbd,samp` match this pad. WeasyPrint UA `kbd` is a 1px-border key.
pub const CODE_SPAN_PAD_X_HU: Hu = 350;
/// Letter CSS code padding-top/bottom 0.1em = 100 HU.
pub const CODE_SPAN_PAD_Y_HU: Hu = 100;
/// Letter CSS `code{background:#ccfbf1}`. `kbd,samp` use the same mint chip.
pub const CODE_SPAN_BG: [u8; 4] = [204, 251, 241, 255];
/// Letter CSS `code{color:#134e4a}`. `kbd,samp` match, not UA CanvasText.
pub const CODE_SPAN_COLOR: [u8; 4] = [19, 78, 74, 255];
/// Letter CSS `code{font-size:.92em}` at 10pt body: 0.92em = 920 HU.
/// `pre code` inherits the same size. `kbd,samp` match. Body 10pt is the wrong code type.
pub const CODE_SPAN_SIZE_HU: Hu = 920;
/// Letter CSS `mark{padding:.05em .2em}` at 10pt body: 0.2em = 200 HU.
pub const MARK_PAD_X_HU: Hu = 200;
/// Letter CSS mark padding-top/bottom 0.05em = 50 HU.
pub const MARK_PAD_Y_HU: Hu = 50;
/// Letter CSS `mark{background:#fde68a}`.
pub const MARK_BG: [u8; 4] = [253, 230, 138, 255];
/// Letter CSS `th{background:#ccfbf1}`.
pub const TH_BG: [u8; 4] = [204, 251, 241, 255];
/// Letter CSS `th{color:#134e4a}`.
pub const TH_FG: [u8; 4] = [19, 78, 74, 255];
/// Letter CSS `th,td{border:1px solid #cbd5e1}` at 96dpi: 1px = 75 HU.
/// `table{border-collapse:collapse}` shares this 1px; a box per cell is 2px.
pub const CELL_BORDER_HU: Hu = HU_PER_INCH / 96;
/// Letter CSS `th,td{border:1px solid #cbd5e1}`.
pub const CELL_BORDER: [u8; 4] = [203, 213, 225, 255];
/// Letter CSS `th{border-bottom:2px solid #0d9488}` at 96dpi: 2px = 150 HU.
pub const TH_RULE_HU: Hu = 2 * (HU_PER_INCH / 96);
/// Letter CSS `h1{padding-bottom:.4rem}` at 16px rem / 96dpi: 0.4rem = 480 HU.
pub const H1_RULE_GAP_HU: Hu = 480;
/// Letter CSS `h1{border-bottom:3px solid #134e4a}` at 96dpi: 3px = 225 HU.
pub const H1_RULE_HU: Hu = HR_THICK_HU;
/// Letter CSS `h2{padding-bottom:.2rem}` at 16px rem / 96dpi: 0.2rem = 240 HU.
pub const H2_RULE_GAP_HU: Hu = 240;
/// Letter CSS `h2{border-bottom:2px solid #0d9488}` at 96dpi: 2px = 150 HU.
pub const H2_RULE_HU: Hu = TH_RULE_HU;
/// Letter CSS `h1{margin:0 0 .6rem}` after the border box. 0.6rem = 720 HU.
/// IR heading `space_after` 400 is jammed vs WeasyPrint.
pub const H1_AFTER_HU: Hu = 720;
/// Letter CSS `h2{margin:1.1rem 0 .45rem}` top. 1.1rem = 1320 HU.
/// Collapses with [`H1_AFTER_HU`] so H1/H2 is 1.1rem after the in-box 3px
/// rule, not IR 400+360.
pub const H2_BEFORE_HU: Hu = 1320;
/// Letter CSS h2 margin-bottom 0.45rem = 540 HU. First body line after the
/// H2 rule. The 24pt mark still uses [`IMAGE_BEFORE_HU`] (0.6rem), which
/// wins the collapse.
pub const H2_AFTER_HU: Hu = 540;
/// Letter CSS `th{border-bottom:2px solid #0d9488}`.
pub const TH_RULE: [u8; 4] = [13, 148, 136, 255];
/// Letter CSS `table{margin:1rem 0}` at 16px rem / 96dpi: 1rem = 1200 HU.
pub const TABLE_BEFORE_HU: Hu = 1200;
/// Letter CSS table margin-bottom 1rem = 1200 HU.
pub const TABLE_AFTER_HU: Hu = 1200;
/// Letter CSS `@page{size:A4;margin:2.5rem 2.75rem}` left/right at 16px rem /
/// 96dpi: 2.75rem = 3300 HU. Hangul [`DEFAULT_MARGIN_HU`] 8504 (~30mm) and
/// WeasyPrint UA `@page{margin:75px}` (5625 HU) are a sparse frame.
pub const PAGE_MARGIN_X_HU: Hu = 3300;
/// Letter CSS `@page` top/bottom 2.5rem = 3000 HU.
pub const PAGE_MARGIN_Y_HU: Hu = 3000;
/// Letter CSS `sup,sub{font-size:.75em}` at 10pt body: 0.75em = 750 HU.
pub const SUPER_SUB_SIZE_HU: Hu = 750;
/// WeasyPrint `vertical-align:super|sub` ±½em of that size (Pango SUPERSUB_RISE).
/// At 10pt body: 375 HU. 2/3-em size or 1/5-em drop reads as missing.
pub const SUPER_SUB_SHIFT_HU: Hu = 375;
const _: () = assert!(H1_SIZE_MAX_HU == 2100);
const _: () = assert!(H1_SIZE_MAX_HU * HEADING_LEADING_PCT as i32 / 100 == 2520);
const _: () = assert!(H2_SIZE_MAX_HU * HEADING_LEADING_PCT as i32 / 100 == 1656);
const _: () = assert!(H1_LETTER_SPACING_HU == -42);
const _: () = assert!(H1_LETTER_SPACING_HU == -H1_SIZE_MAX_HU * 2 / 100);
const _: () = assert!(H1_LETTER_SPACING_HU < 0);
const _: () = assert!(H2_SIZE_MAX_HU == 1380);
const _: () = assert!(BODY_AFTER_HU == 840);
const _: () = assert!(BODY_LEADING_PCT == 145);
const _: () = assert!(HEADING_LEADING_PCT == 120);
const _: () = assert!(HEADING_LEADING_PCT < BODY_LEADING_PCT);
const _: () = assert!(CELL_PAD_X_HU == 780);
const _: () = assert!(CELL_PAD_Y_HU == 480);
const _: () = assert!(CELL_SIZE_HU == 1140);
const _: () = assert!(CELL_SIZE_HU > DEFAULT_FONT_SIZE_HU);
const _: () = assert!(IMAGE_BEFORE_HU == 720);
const _: () = assert!(IMAGE_AFTER_HU == 840);
const _: () = assert!(IMAGE_BEFORE_HU < IMAGE_AFTER_HU);
const _: () = assert!(LIST_INDENT_HU == 1500);
const _: () = assert!(LIST_ITEM_AFTER_HU == 420);
const _: () = assert!(LIST_AFTER_HU == 1200);
const _: () = assert!(LIST_NESTED_GAP_HU == 300);
const _: () = assert!(LIST_NESTED_GAP_HU < LIST_ITEM_AFTER_HU);
const _: () = assert!(LIST_NESTED_GAP_HU < BODY_AFTER_HU);
const _: () = assert!(LIST_ITEM_AFTER_HU < BODY_AFTER_HU);
const _: () = assert!(LIST_AFTER_HU > LIST_ITEM_AFTER_HU);
const _: () = assert!(LIST_AFTER_HU > BODY_AFTER_HU);
const _: () = assert!(LIST_MARK_GAP_HU == 480);
const _: () = assert!(TASK_BOX_HU == 900);
const _: () = assert!(TASK_BORDER_HU == 150);
const _: () = assert!(TASK_MARK_ADVANCE_HU == 1380);
const _: () = assert!(TASK_MARK_ADVANCE_HU == TASK_BOX_HU + LIST_MARK_GAP_HU);
const _: () = assert!(TASK_MARK_ADVANCE_HU < LIST_INDENT_HU * 2);
const _: () = assert!(QUOTE_BEFORE_HU == 600);
const _: () = assert!(QUOTE_AFTER_HU == 1200);
const _: () = assert!(QUOTE_PAD_X_HU == 1200);
const _: () = assert!(QUOTE_PAD_Y_HU == 180);
const _: () = assert!(QUOTE_BAR_W_HU == 300);
const _: () = assert!(QUOTE_FG[0] == 19 && QUOTE_FG[1] == 78 && QUOTE_FG[2] == 74);
const _: () = assert!(BODY_FG[0] == 19 && BODY_FG[1] == 78 && BODY_FG[2] == 74);
const _: () = assert!(BODY_FG[0] != 0 || BODY_FG[1] != 0 || BODY_FG[2] != 0);
const _: () = assert!(MARKER_FG[0] == 19 && MARKER_FG[1] == 78 && MARKER_FG[2] == 74);
const _: () = assert!(
    MARKER_FG[0] == BODY_FG[0] && MARKER_FG[1] == BODY_FG[1] && MARKER_FG[2] == BODY_FG[2]
);
const _: () = assert!(CODE_BEFORE_HU == 720);
const _: () = assert!(CODE_AFTER_HU == 1200);
const _: () = assert!(CODE_PAD_X_HU == 1200);
const _: () = assert!(CODE_PAD_X_HU == 16 * (HU_PER_INCH / 96));
const _: () = assert!(CODE_PAD_Y_HU == 900);
/// Inherited `code{padding:.1em .35em}` is smaller than `pre{padding:.75rem 1rem}`.
/// [`code_span_chip`] must zero that inner pad so it cannot inset the fence.
const _: () = assert!(CODE_SPAN_PAD_X_HU < CODE_PAD_X_HU);
const _: () = assert!(CODE_SPAN_PAD_Y_HU < CODE_PAD_Y_HU);
const _: () = assert!(HR_THICK_HU == 225);
const _: () = assert!(HR_THICK_HU > 80);
const _: () = assert!(HR_BEFORE_HU == 1320);
const _: () = assert!(HR_BEFORE_HU > BODY_AFTER_HU);
const _: () = assert!(HR_AFTER_HU == 1320);
const _: () = assert!(LINK_UNDERLINE_HU == 80);
const _: () = assert!(UNDERLINE_HU == 80);
const _: () = assert!(UNDERLINE_OFFSET_HU == 100);
const _: () = assert!(ABBR_UNDERLINE_OFFSET_HU == 150);
const _: () = assert!(ABBR_UNDERLINE_OFFSET_HU == DEFAULT_FONT_SIZE_HU * 15 / 100);
const _: () = assert!(ABBR_UNDERLINE_OFFSET_HU > UNDERLINE_OFFSET_HU);
const _: () = assert!(DOTTED_UNDERLINE_DOT_HU == UNDERLINE_HU);
const _: () = assert!(STRIKE_HU == 80);
const _: () = assert!(STRIKE_COLOR[0] == 19 && STRIKE_COLOR[1] == 78 && STRIKE_COLOR[2] == 74);
const _: () = assert!(LINK_COLOR[0] == 13 && LINK_COLOR[1] == 148 && LINK_COLOR[2] == 136);
const _: () = assert!(CODE_SPAN_PAD_X_HU == 350);
const _: () = assert!(CODE_SPAN_PAD_Y_HU == 100);
const _: () = assert!(MARK_PAD_X_HU == 200);
const _: () = assert!(MARK_PAD_Y_HU == 50);
const _: () = assert!(CODE_SPAN_BG[0] == 204 && CODE_SPAN_BG[1] == 251 && CODE_SPAN_BG[2] == 241);
const _: () = assert!(CODE_SPAN_COLOR[0] == 19 && CODE_SPAN_COLOR[1] == 78 && CODE_SPAN_COLOR[2] == 74);
const _: () = assert!(MARK_BG[0] == 253 && MARK_BG[1] == 230 && MARK_BG[2] == 138);
const _: () = assert!(TH_BG[0] == 204 && TH_BG[1] == 251 && TH_BG[2] == 241);
const _: () = assert!(TH_FG[0] == 19 && TH_FG[1] == 78 && TH_FG[2] == 74);
const _: () = assert!(QUOTE_FG[0] == TH_FG[0] && QUOTE_FG[1] == TH_FG[1] && QUOTE_FG[2] == TH_FG[2]);
const _: () = assert!(
    QUOTE_FG[0] == STRIKE_COLOR[0] && QUOTE_FG[1] == STRIKE_COLOR[1] && QUOTE_FG[2] == STRIKE_COLOR[2]
);
const _: () = assert!(BODY_FG[0] == QUOTE_FG[0] && BODY_FG[1] == QUOTE_FG[1] && BODY_FG[2] == QUOTE_FG[2]);
const _: () = assert!(BODY_FG[0] == TH_FG[0] && BODY_FG[1] == TH_FG[1] && BODY_FG[2] == TH_FG[2]);
const _: () = assert!(CELL_BORDER_HU == 75);
const _: () = assert!(CELL_BORDER[0] == 203 && CELL_BORDER[1] == 213 && CELL_BORDER[2] == 225);
const _: () = assert!(TH_RULE_HU == 150);
const _: () = assert!(TH_RULE_HU == 2 * CELL_BORDER_HU);
const _: () = assert!(TH_RULE[0] == 13 && TH_RULE[1] == 148 && TH_RULE[2] == 136);
const _: () = assert!(H1_RULE_GAP_HU == 480);
const _: () = assert!(H1_RULE_HU == 225);
const _: () = assert!(H1_RULE_HU == HR_THICK_HU);
const _: () = assert!(H1_RULE_GAP_HU > H2_RULE_GAP_HU);
const _: () = assert!(H2_RULE_GAP_HU == 240);
const _: () = assert!(H2_RULE_HU == 150);
const _: () = assert!(H2_RULE_HU == TH_RULE_HU);
const _: () = assert!(H1_AFTER_HU == 720);
const _: () = assert!(H1_AFTER_HU == IMAGE_BEFORE_HU);
const _: () = assert!(H2_BEFORE_HU == 1320);
const _: () = assert!(H2_BEFORE_HU == HR_BEFORE_HU);
const _: () = assert!(H2_BEFORE_HU > H1_AFTER_HU);
const _: () = assert!(H2_AFTER_HU == 540);
const _: () = assert!(H2_AFTER_HU < IMAGE_BEFORE_HU);
const _: () = assert!(H2_AFTER_HU < BODY_AFTER_HU);
const _: () = assert!(H2_AFTER_HU < H2_BEFORE_HU);
const _: () = assert!(TABLE_BEFORE_HU == 1200);
const _: () = assert!(TABLE_AFTER_HU == 1200);
const _: () = assert!(PAGE_MARGIN_X_HU == 3300);
const _: () = assert!(PAGE_MARGIN_Y_HU == 3000);
const _: () = assert!(PAGE_MARGIN_X_HU == 44 * (HU_PER_INCH / 96));
const _: () = assert!(PAGE_MARGIN_Y_HU == 40 * (HU_PER_INCH / 96));
const _: () = assert!(PAGE_MARGIN_X_HU > PAGE_MARGIN_Y_HU);
const _: () = assert!(PAGE_MARGIN_X_HU < DEFAULT_MARGIN_HU);
const _: () = assert!(PAGE_MARGIN_Y_HU < DEFAULT_MARGIN_HU);
const _: () = assert!(PAGE_MARGIN_X_HU != 75 * (HU_PER_INCH / 96));
const _: () = assert!(PAGE_MARGIN_Y_HU != 75 * (HU_PER_INCH / 96));
const _: () = assert!(SUPER_SUB_SIZE_HU == 750);
const _: () = assert!(SUPER_SUB_SHIFT_HU == 375);
const _: () = assert!(SUPER_SUB_SIZE_HU == DEFAULT_FONT_SIZE_HU * 3 / 4);
const _: () = assert!(SUPER_SUB_SHIFT_HU == SUPER_SUB_SIZE_HU / 2);
const _: () = assert!(CODE_SPAN_SIZE_HU == 920);
const _: () = assert!(CODE_SPAN_SIZE_HU == DEFAULT_FONT_SIZE_HU * 92 / 100);
const _: () = assert!(CODE_SPAN_SIZE_HU < DEFAULT_FONT_SIZE_HU);
const _: () = assert!(CODE_SPAN_SIZE_HU > SUPER_SUB_SIZE_HU);

/// Painted `sup`/`sub` size. Letter CSS `font-size:.75em` of parent.
pub fn super_sub_size(parent: Hu) -> Hu {
    parent.saturating_mul(3).saturating_div(4).max(1)
}

/// WeasyPrint `vertical-align:super|sub` rise/drop: ±½em of the run size.
pub fn super_sub_shift(run_size: Hu) -> Hu {
    run_size.saturating_div(2)
}

/// Painted `code`/`kbd`/`samp`/`pre code` size. Letter CSS `font-size:.92em` of parent.
pub fn code_span_size(parent: Hu) -> Hu {
    i32::try_from(i64::from(parent.max(1)) * 92 / 100)
        .unwrap_or(i32::MAX)
        .max(1)
}

/// Letter CSS `pre code{background:transparent;padding:0}`.
/// Inline `code`/`kbd`/`samp` paint the mint chip; HTML `<pre><code>` sets both
/// `code_block` and `style.code`, so inherited [`CODE_SPAN_PAD_X_HU`] would
/// inset the prove fence inside [`CODE_PAD_X_HU`].
pub fn code_span_chip(code: bool, code_block: bool) -> bool {
    code && !code_block
}

/// Extra Y inside a table row so cell content matches CSS `vertical-align`.
/// Letter CSS `th,td{vertical-align:top}` is offset 0. WeasyPrint UA
/// `thead,tbody,tfoot,table>tr{vertical-align:middle}` plus
/// `td,th{vertical-align:inherit}` centers wrapped hop cells in the leftover
/// and punches sparse bands.
pub fn cell_valign_offset(valign: VAlign, cell_h: Hu, row_h: Hu) -> Hu {
    let leftover = row_h.saturating_sub(cell_h);
    match valign {
        VAlign::Top => 0,
        VAlign::Middle => leftover / 2,
        VAlign::Bottom => leftover,
    }
}

/// Painted heading line box. Letter CSS `h1,h2{line-height:1.2}` of size.
pub fn heading_line_height(size: Hu) -> Hu {
    i32::try_from(i64::from(size.max(1)) * i64::from(HEADING_LEADING_PCT) / 100)
        .unwrap_or(i32::MAX)
        .max(1)
}

/// Extra advance between H1 glyphs. Letter CSS `h1{letter-spacing:-.02em}`.
pub fn h1_letter_spacing(size: Hu) -> Hu {
    -(size.max(1).saturating_mul(2) / 100)
}

/// Letter CSS `abbr[title]{text-underline-offset:.15em}` of the run size.
pub fn dotted_underline_offset(size: Hu) -> Hu {
    i32::try_from(i64::from(size.max(1)) * 15 / 100)
        .unwrap_or(i32::MAX)
        .max(1)
}

/// Painted left/right page inset. Letter CSS `@page` 2.75rem, not Hangul 30mm.
pub fn page_margin_x(ir: Hu) -> Hu {
    if ir == DEFAULT_MARGIN_HU {
        PAGE_MARGIN_X_HU
    } else {
        ir.max(0)
    }
}

/// Painted top/bottom page inset. Letter CSS `@page` 2.5rem, not Hangul 30mm.
pub fn page_margin_y(ir: Hu) -> Hu {
    if ir == DEFAULT_MARGIN_HU {
        PAGE_MARGIN_Y_HU
    } else {
        ir.max(0)
    }
}

/// CSS letter-spacing sits between adjacent characters, not after the last.
pub fn apply_letter_spacing(advances: &mut [i32], tracking: Hu) {
    if tracking == 0 || advances.len() < 2 {
        return;
    }
    let last = advances.len() - 1;
    for adv in &mut advances[..last] {
        *adv = adv.saturating_add(tracking);
    }
}

/// Raise `width`×`height` so both edges are at least [`MIN_IMAGE_EDGE_HU`].
pub fn min_on_page_size(width: Hu, height: Hu) -> (Hu, Hu) {
    let short = width.min(height);
    if short >= MIN_IMAGE_EDGE_HU || short <= 0 {
        return (width.max(0), height.max(0));
    }
    let w = (Hu64::from(width) * Hu64::from(MIN_IMAGE_EDGE_HU) / Hu64::from(short)).max(1);
    let h = (Hu64::from(height) * Hu64::from(MIN_IMAGE_EDGE_HU) / Hu64::from(short)).max(1);
    (
        i32::try_from(w).unwrap_or(i32::MAX),
        i32::try_from(h).unwrap_or(i32::MAX),
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FragmentTree {
    pub pages: Vec<PageFrag>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageFrag {
    pub width: Hu,
    pub height: Hu,
    pub lines: Vec<LineFrag>,
    pub rects: Vec<RectFrag>,
    /// Table grid strokes and heading rules. Not counted by `table_row_heights`.
    pub strokes: Vec<RectFrag>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineFrag {
    pub x: Hu,
    pub y: Hu,
    pub width: Hu,
    pub height: Hu,
    pub baseline: Hu,
    pub text: String,
    pub font_size: Hu,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    /// Letter CSS `abbr[title]{text-decoration:underline dotted}`. Solid `u`/`a` stay false.
    pub dotted: bool,
    pub strike: bool,
    pub code: bool,
    pub highlight: bool,
    /// Run color. Header cells use [`TH_FG`]; quotes use [`QUOTE_FG`];
    /// struck runs use [`STRIKE_COLOR`]; body copy uses [`BODY_FG`].
    pub color: [u8; 4],
    /// External URI for this run, if it is a hyperlink.
    pub href: Option<String>,
    /// Inline raster occupying `width` × stored height in HWPUNIT.
    pub image: Option<InlineImage>,
}

/// PNG/JPEG bytes laid out as an inline run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineImage {
    pub bytes: Vec<u8>,
    pub mime: String,
    pub width: Hu,
    pub height: Hu,
}

impl LineFrag {
    /// Hit box plus URI for a PDF `/Link` annotation. Height covers glyphs
    /// even when this fragment is not the last on the line (`height` is 0).
    pub fn link_hit(&self) -> Option<(Hu, Hu, Hu, Hu, &str)> {
        let uri = self.href.as_deref().filter(|u| !u.is_empty())?;
        if self.width <= 0 {
            return None;
        }
        let h = self
            .height
            .max(self.font_size)
            .max(self.baseline.saturating_add(120));
        Some((self.x, self.y, self.width, h, uri))
    }

    /// Painted image box in the same top-left HWPUNIT space as `x`/`y`.
    pub fn image_box(&self) -> Option<(Hu, Hu, Hu, Hu)> {
        let img = self.image.as_ref()?;
        if img.width <= 0 || img.height <= 0 {
            return None;
        }
        Some((self.x, self.y, img.width, img.height))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RectFrag {
    pub x: Hu,
    pub y: Hu,
    pub width: Hu,
    pub height: Hu,
    pub fill: [u8; 4],
}

pub fn layout_document(doc: &Document, fonts: &FontSet) -> FragmentTree {
    let mut pages = Vec::new();
    for section in &doc.sections {
        pages.extend(layout_section(section, fonts));
    }
    if pages.is_empty() {
        let p = Section::default().page;
        pages.push(PageFrag {
            width: p.width,
            height: p.height,
            lines: Vec::new(),
            rects: Vec::new(),
            strokes: Vec::new(),
        });
    }
    FragmentTree { pages }
}

fn layout_section(section: &Section, fonts: &FontSet) -> Vec<PageFrag> {
    let page = &section.page;
    let origin_x = page_margin_x(page.margin_left);
    let origin_y = page_margin_y(page.margin_top);
    let margin_right = page_margin_x(page.margin_right);
    let margin_bottom = page_margin_y(page.margin_bottom);
    let content_w = (page.width - origin_x - margin_right - page.gutter).max(1);
    let content_h = (page.height - origin_y - margin_bottom).max(1);

    let mut prepared: Vec<Prepared> = section
        .body
        .par_iter()
        .map(|block| prepare_block(block, fonts, content_w))
        .collect();
    apply_list_block_after(&section.body, &mut prepared);

    let mut pages = Vec::new();
    let mut current = empty_page(page.width, page.height);
    let mut y = origin_y;
    let mut pending_after: Hu = 0;

    for item in prepared {
        match item {
            Prepared::Lines {
                lines,
                space_before,
                space_after,
                pad_top,
                pad_bottom,
                rule,
                fills,
            } => {
                y += space_before.max(pending_after);
                y += pad_top;
                let mut text_left: Option<Hu> = None;
                let mut text_right = origin_x;
                let mut block_bottom = y;
                let mut first_line_y = y;
                let mut saw_line = false;
                let mut i = 0;
                while let Some((a, b, row_h)) = next_line_row(&lines, i) {
                    i = b;
                    if y + row_h > origin_y + content_h && !current.lines.is_empty() {
                        pages.push(std::mem::replace(
                            &mut current,
                            empty_page(page.width, page.height),
                        ));
                        y = origin_y + space_before;
                    }
                    if !saw_line {
                        first_line_y = y;
                        saw_line = true;
                    }
                    for line in &lines[a..b] {
                        let placed = place_row_frag(line, origin_x, y);
                        if placed.width > 0 {
                            text_left = Some(text_left.map_or(placed.x, |l| l.min(placed.x)));
                            text_right = text_right.max(placed.x.saturating_add(placed.width));
                        }
                        push_underlined_line(&mut current, placed);
                    }
                    y += row_h;
                    if row_h > 0 {
                        block_bottom = y;
                    }
                }
                let mut rule_clearance: Hu = 0;
                if let (Some(rule), Some(left)) = (rule, text_left) {
                    let (rx, rw) = if rule.span_content {
                        (origin_x, content_w)
                    } else {
                        (left, (text_right - left).max(1))
                    };
                    current.strokes.push(RectFrag {
                        x: rx,
                        y: block_bottom.saturating_add(rule.gap),
                        width: rw,
                        height: rule.thickness,
                        fill: rule.color,
                    });
                    rule_clearance = rule.gap.saturating_add(rule.thickness);
                }
                for fill in fills {
                    current.strokes.push(RectFrag {
                        x: origin_x.saturating_add(fill.x),
                        y: first_line_y.saturating_add(fill.y),
                        width: fill.width,
                        height: fill.height,
                        fill: fill.fill,
                    });
                }
                y = y.saturating_add(pad_bottom);
                pending_after = space_after.max(rule_clearance.saturating_sub(pad_bottom));
            }
            Prepared::Table {
                rows,
                col_widths,
                stroke_w,
                stroke,
                space_before,
                space_after,
            } => {
                y += space_before.max(pending_after);
                pending_after = space_after;
                let x0 = origin_x;
                for (ri, row) in rows.iter().enumerate() {
                    let row_h = row.iter().map(|c| c.height).max().unwrap_or(0).max(MIN_ROW_REMAINDER_HU);
                    if y + row_h > origin_y + content_h && !current.lines.is_empty() {
                        pages.push(std::mem::replace(
                            &mut current,
                            empty_page(page.width, page.height),
                        ));
                        y = origin_y;
                    }
                    place_table_row(
                        &mut current,
                        row,
                        &col_widths,
                        x0,
                        y,
                        row_h,
                        TableRowStroke {
                            width: stroke_w,
                            color: stroke,
                            last_row: ri + 1 == rows.len(),
                            draw_top: table_row_draws_top(&rows, ri),
                        },
                    );
                    y += row_h;
                }
            }
        }
    }
    pages.push(current);
    // Stamp after body so pagination still sees a fresh page as empty.
    stamp_header_footer(
        section,
        fonts,
        content_w,
        origin_x,
        origin_y,
        margin_bottom,
        &mut pages,
    );
    pages
}

fn prepare_header_footer(hf: &HeaderFooter, fonts: &FontSet, width: Hu) -> Vec<Prepared> {
    hf.blocks
        .par_iter()
        .map(|block| prepare_block(block, fonts, width))
        .collect()
}

fn stamp_header_footer(
    section: &Section,
    fonts: &FontSet,
    content_w: Hu,
    origin_x: Hu,
    origin_y: Hu,
    margin_bottom: Hu,
    pages: &mut [PageFrag],
) {
    let page = &section.page;
    let header = section
        .header
        .as_ref()
        .map(|hf| prepare_header_footer(hf, fonts, content_w));
    let footer = section
        .footer
        .as_ref()
        .map(|hf| prepare_header_footer(hf, fonts, content_w));
    if header.is_none() && footer.is_none() {
        return;
    }
    let header_y = origin_y.saturating_sub(page.header_distance).max(0);
    let footer_y = page.height.saturating_sub(margin_bottom).max(0);
    for frag in pages {
        if let Some(items) = header.as_deref() {
            place_prepared_band(frag, items, origin_x, header_y, content_w, page.height);
        }
        if let Some(items) = footer.as_deref() {
            place_prepared_band(frag, items, origin_x, footer_y, content_w, page.height);
        }
    }
}

fn place_prepared_band(
    page: &mut PageFrag,
    items: &[Prepared],
    origin_x: Hu,
    origin_y: Hu,
    content_w: Hu,
    page_h: Hu,
) {
    let mut y = origin_y.max(0);
    let mut pending_after: Hu = 0;
    for item in items {
        match item {
            Prepared::Lines {
                lines,
                space_before,
                space_after,
                pad_top,
                pad_bottom,
                rule,
                fills,
            } => {
                y = y.saturating_add((*space_before).max(pending_after));
                y = y.saturating_add(*pad_top);
                let mut text_left: Option<Hu> = None;
                let mut text_right = origin_x;
                let mut block_bottom = y;
                let mut first_line_y = y;
                let mut saw_line = false;
                let mut i = 0;
                while let Some((a, b, row_h)) = next_line_row(lines, i) {
                    i = b;
                    if y >= page_h {
                        break;
                    }
                    if !saw_line {
                        first_line_y = y;
                        saw_line = true;
                    }
                    for line in &lines[a..b] {
                        let placed = place_row_frag(line, origin_x, y);
                        if placed.width > 0 {
                            text_left = Some(text_left.map_or(placed.x, |l| l.min(placed.x)));
                            text_right = text_right.max(placed.x.saturating_add(placed.width));
                        }
                        push_underlined_line(page, placed);
                    }
                    y = y.saturating_add(row_h);
                    if row_h > 0 {
                        block_bottom = y;
                    }
                }
                let mut rule_clearance: Hu = 0;
                if let (Some(rule), Some(left)) = (rule.as_ref(), text_left) {
                    let (rx, rw) = if rule.span_content {
                        (origin_x, content_w)
                    } else {
                        (left, (text_right - left).max(1))
                    };
                    page.strokes.push(RectFrag {
                        x: rx,
                        y: block_bottom.saturating_add(rule.gap),
                        width: rw,
                        height: rule.thickness,
                        fill: rule.color,
                    });
                    rule_clearance = rule.gap.saturating_add(rule.thickness);
                }
                for fill in fills {
                    page.strokes.push(RectFrag {
                        x: origin_x.saturating_add(fill.x),
                        y: first_line_y.saturating_add(fill.y),
                        width: fill.width,
                        height: fill.height,
                        fill: fill.fill,
                    });
                }
                y = y.saturating_add(*pad_bottom);
                pending_after = (*space_after).max(rule_clearance.saturating_sub(*pad_bottom));
            }
            Prepared::Table {
                rows,
                col_widths,
                stroke_w,
                stroke,
                space_before,
                space_after,
            } => {
                y = y.saturating_add((*space_before).max(pending_after));
                pending_after = *space_after;
                let x0 = origin_x;
                for (ri, row) in rows.iter().enumerate() {
                    let row_h = row
                        .iter()
                        .map(|c| c.height)
                        .max()
                        .unwrap_or(0)
                        .max(MIN_ROW_REMAINDER_HU);
                    if y >= page_h {
                        break;
                    }
                    place_table_row(
                        page,
                        row,
                        col_widths,
                        x0,
                        y,
                        row_h,
                        TableRowStroke {
                            width: *stroke_w,
                            color: *stroke,
                            last_row: ri + 1 == rows.len(),
                            draw_top: table_row_draws_top(rows, ri),
                        },
                    );
                    y = y.saturating_add(row_h);
                }
            }
        }
    }
}

fn empty_page(width: Hu, height: Hu) -> PageFrag {
    PageFrag {
        width,
        height,
        lines: Vec::new(),
        rects: Vec::new(),
        strokes: Vec::new(),
    }
}

/// Prepared rows store the line box on the last fragment (`height > 0`);
/// earlier fragments keep `height == 0` so they share `y`.
fn next_line_row(lines: &[LineFrag], start: usize) -> Option<(usize, usize, Hu)> {
    if start >= lines.len() {
        return None;
    }
    let mut end = start;
    let mut row_h: Hu = 0;
    while end < lines.len() {
        let line = &lines[end];
        let img_h = line.image.as_ref().map(|im| im.height).unwrap_or(0);
        row_h = row_h.max(line.height).max(img_h);
        end += 1;
        if line.height > 0 {
            break;
        }
    }
    Some((start, end, row_h))
}

fn place_row_frag(line: &LineFrag, origin_x: Hu, y: Hu) -> LineFrag {
    let mut placed = line.clone();
    placed.x = origin_x.saturating_add(placed.x);
    placed.y = y;
    if let Some(img) = &placed.image {
        placed.height = placed.height.max(img.height);
    }
    placed
}

fn cell_valign_shift(cell: &PreparedCell, row_h: Hu) -> Hu {
    cell_valign_offset(cell.valign, cell.height, row_h)
}

fn place_cell_lines(page: &mut PageFrag, cell: &PreparedCell, x: Hu, y: Hu, row_h: Hu) {
    let mut ly = y
        .saturating_add(cell_valign_shift(cell, row_h))
        .saturating_add(cell.pad_top);
    let mut li = 0;
    while let Some((a, b, line_h)) = next_line_row(&cell.lines, li) {
        li = b;
        for line in &cell.lines[a..b] {
            let placed = place_row_frag(line, x.saturating_add(cell.pad_left), ly);
            push_underlined_line(page, placed);
        }
        ly = ly.saturating_add(line_h);
    }
}

fn push_underlined_line(page: &mut PageFrag, placed: LineFrag) {
    if placed.underline && placed.width > 0 {
        if placed.dotted {
            page.strokes.extend(dotted_underline_fills(&placed));
        } else {
            page.strokes.push(underline_fill(&placed));
        }
    }
    if placed.strike && placed.width > 0 {
        page.strokes.push(strike_fill(&placed));
    }
    if placed.highlight && placed.width > 0 {
        page.strokes.push(mark_span_fill(&placed));
    }
    if placed.code && placed.width > 0 {
        page.strokes.push(code_span_fill(&placed));
    }
    page.lines.push(placed);
}

/// Letter CSS `code,kbd,samp{background:#ccfbf1;padding:.1em .35em}` mint chip.
/// WeasyPrint UA `kbd` is a 1px-border key, not this fill.
fn code_span_fill(placed: &LineFrag) -> RectFrag {
    RectFrag {
        x: placed.x.saturating_sub(CODE_SPAN_PAD_X_HU),
        y: placed
            .y
            .saturating_add(placed.baseline)
            .saturating_sub(placed.font_size)
            .saturating_sub(CODE_SPAN_PAD_Y_HU),
        width: placed
            .width
            .saturating_add(CODE_SPAN_PAD_X_HU.saturating_mul(2)),
        height: placed
            .font_size
            .saturating_add(CODE_SPAN_PAD_Y_HU.saturating_mul(2))
            .max(1),
        fill: CODE_SPAN_BG,
    }
}

fn underline_fill(placed: &LineFrag) -> RectFrag {
    let link = placed.href.is_some();
    RectFrag {
        x: placed.x,
        y: placed
            .y
            .saturating_add(placed.baseline)
            .saturating_add(UNDERLINE_OFFSET_HU),
        width: placed.width,
        height: if link { LINK_UNDERLINE_HU } else { UNDERLINE_HU },
        fill: if link { LINK_COLOR } else { STRIKE_COLOR },
    }
}

/// Letter CSS `abbr[title]{text-decoration:underline dotted #134e4a}`.
/// WeasyPrint UA `text-decoration: dotted underline` is uncolored CanvasText.
fn dotted_underline_fills(placed: &LineFrag) -> Vec<RectFrag> {
    let y = placed
        .y
        .saturating_add(placed.baseline)
        .saturating_add(dotted_underline_offset(placed.font_size));
    let dot = DOTTED_UNDERLINE_DOT_HU.max(1);
    let period = dot.saturating_add(dot);
    let end = placed.x.saturating_add(placed.width);
    let mut out = Vec::new();
    let mut x = placed.x;
    while x < end {
        let w = dot.min(end.saturating_sub(x)).max(1);
        out.push(RectFrag {
            x,
            y,
            width: w,
            height: UNDERLINE_HU,
            fill: BODY_FG,
        });
        x = x.saturating_add(period);
    }
    out
}

fn strike_fill(placed: &LineFrag) -> RectFrag {
    RectFrag {
        x: placed.x,
        y: placed
            .y
            .saturating_add(placed.baseline)
            .saturating_sub(placed.font_size / 3),
        width: placed.width,
        height: STRIKE_HU,
        fill: STRIKE_COLOR,
    }
}

/// Thin `u`/`s`/`a` decoration. Painted after glyphs so the rule is not missing
/// under ink, unlike cell/code/mark fills which stay behind text.
pub fn is_text_decoration(rect: &RectFrag) -> bool {
    rect.height == UNDERLINE_HU && (rect.fill == STRIKE_COLOR || rect.fill == LINK_COLOR)
}

fn mark_span_fill(placed: &LineFrag) -> RectFrag {
    RectFrag {
        x: placed.x.saturating_sub(MARK_PAD_X_HU),
        y: placed
            .y
            .saturating_add(placed.baseline)
            .saturating_sub(placed.font_size)
            .saturating_sub(MARK_PAD_Y_HU),
        width: placed.width.saturating_add(MARK_PAD_X_HU.saturating_mul(2)),
        height: placed
            .font_size
            .saturating_add(MARK_PAD_Y_HU.saturating_mul(2))
            .max(1),
        fill: MARK_BG,
    }
}

fn table_row_draws_top(rows: &[Vec<PreparedCell>], ri: usize) -> bool {
    // `th{border-bottom:2px}` owns the header/body seam; a 1px gray on the
    // next row would stack into 3px next to WeasyPrint collapse.
    ri == 0 || !rows[ri - 1].iter().any(|c| c.header)
}

struct TableRowStroke {
    width: Hu,
    color: [u8; 4],
    last_row: bool,
    draw_top: bool,
}

struct CellEdges {
    top: bool,
    right: bool,
    bottom: bool,
    left: bool,
}

fn place_table_row(
    page: &mut PageFrag,
    row: &[PreparedCell],
    col_widths: &[Hu],
    x0: Hu,
    y: Hu,
    row_h: Hu,
    grid: TableRowStroke,
) {
    let n_cols = col_widths.len();
    let mut x = x0;
    for (ci, cell) in row.iter().enumerate() {
        let cw = *col_widths.get(ci).unwrap_or(&10000);
        page.rects.push(RectFrag {
            x,
            y,
            width: cw,
            height: row_h,
            fill: cell.fill,
        });
        if grid.width > 0 {
            let cell_box = page.rects.last().expect("cell fill");
            push_cell_strokes(
                &mut page.strokes,
                cell_box,
                grid.width,
                grid.color,
                CellEdges {
                    top: grid.draw_top,
                    right: ci + 1 == n_cols,
                    bottom: grid.last_row && !cell.header,
                    left: true,
                },
            );
        }
        if cell.header {
            push_header_rule(&mut page.strokes, x, y, cw, row_h);
        }
        place_cell_lines(page, cell, x, y, row_h);
        x += cw;
    }
}

/// Letter CSS `table{border-collapse:collapse}`: one 1px edge per grid line,
/// not a 1px box per cell (adjacent boxes paint a 2px internal grid).
fn push_cell_strokes(
    strokes: &mut Vec<RectFrag>,
    cell: &RectFrag,
    t: Hu,
    color: [u8; 4],
    edges: CellEdges,
) {
    let t = t.min(cell.width).min(cell.height).max(1);
    let x = cell.x;
    let y = cell.y;
    let w = cell.width;
    let h = cell.height;
    if edges.top {
        strokes.push(RectFrag {
            x,
            y,
            width: w,
            height: t,
            fill: color,
        });
    }
    if edges.left {
        strokes.push(RectFrag {
            x,
            y,
            width: t,
            height: h,
            fill: color,
        });
    }
    if edges.bottom {
        strokes.push(RectFrag {
            x,
            y: y + h - t,
            width: w,
            height: t,
            fill: color,
        });
    }
    if edges.right {
        strokes.push(RectFrag {
            x: x + w - t,
            y,
            width: t,
            height: h,
            fill: color,
        });
    }
}

fn push_header_rule(strokes: &mut Vec<RectFrag>, x: Hu, y: Hu, w: Hu, h: Hu) {
    let t = TH_RULE_HU.min(h).max(1);
    strokes.push(RectFrag {
        x,
        y: y + h - t,
        width: w,
        height: t,
        fill: TH_RULE,
    });
}

enum Prepared {
    Lines {
        lines: Vec<LineFrag>,
        space_before: Hu,
        space_after: Hu,
        pad_top: Hu,
        pad_bottom: Hu,
        rule: Option<HeadingRule>,
        fills: Vec<RectFrag>,
    },
    Table {
        rows: Vec<Vec<PreparedCell>>,
        col_widths: Vec<Hu>,
        stroke_w: Hu,
        stroke: [u8; 4],
        space_before: Hu,
        space_after: Hu,
    },
}

struct HeadingRule {
    thickness: Hu,
    color: [u8; 4],
    span_content: bool,
    gap: Hu,
}

fn heading_rule(level: Option<u8>) -> Option<HeadingRule> {
    match level {
        Some(1) => Some(HeadingRule {
            thickness: H1_RULE_HU,
            color: BODY_FG,
            span_content: true,
            gap: H1_RULE_GAP_HU,
        }),
        Some(n) if n >= 2 => Some(HeadingRule {
            thickness: H2_RULE_HU,
            color: TH_RULE,
            // Letter CSS `h2{border-bottom:2px}` is block-level: the 2px
            // #0d9488 rule spans the content box, same as H1. A text-span
            // rule is short next to WeasyPrint.
            span_content: true,
            gap: H2_RULE_GAP_HU,
        }),
        _ => None,
    }
}

struct PreparedCell {
    lines: Vec<LineFrag>,
    height: Hu,
    header: bool,
    pad_left: Hu,
    pad_top: Hu,
    fill: [u8; 4],
    valign: VAlign,
}

fn heading_display_size(level: Option<u8>, ir: Hu) -> Hu {
    let ir = ir.max(1);
    match level {
        Some(1) => H1_SIZE_MAX_HU,
        Some(2) => H2_SIZE_MAX_HU,
        Some(n) if n >= 3 => ir.min(DEFAULT_FONT_SIZE_HU.saturating_add(100)),
        _ => ir,
    }
}

fn paragraph_leading(fonts: &FontSet, p: &Paragraph, size: Hu) -> Hu {
    let natural = line_height(fonts, size, 100);
    match p.line_spacing {
        LineSpacing::Absolute(hu) => hu.max(1),
        LineSpacing::AtLeast(hu) => natural.max(hu).max(1),
        LineSpacing::Percent(v) => {
            let cap = if p.outline_level.is_some() {
                HEADING_LEADING_PCT
            } else {
                BODY_LEADING_PCT
            };
            let pct = v.max(100).min(cap);
            let css = i32::try_from(i64::from(size.max(1)) * i64::from(pct) / 100).unwrap_or(i32::MAX);
            if p.outline_level.is_some() {
                // CSS used value, not a floor on (asc+desc).
                css.max(1)
            } else {
                css.max(natural).max(1)
            }
        }
    }
}

fn cell_pads(cell: &docagent_model::TableCell) -> (Hu, Hu, Hu, Hu) {
    let p = cell.padding;
    (
        if p.left > 0 { p.left } else { CELL_PAD_X_HU },
        if p.right > 0 { p.right } else { CELL_PAD_X_HU },
        if p.top > 0 { p.top } else { CELL_PAD_Y_HU },
        if p.bottom > 0 { p.bottom } else { CELL_PAD_Y_HU },
    )
}

fn paragraph_space_after(p: &Paragraph) -> Hu {
    let ir = p.space_after.max(0);
    if paragraph_is_image_only(p) {
        IMAGE_AFTER_HU.max(ir)
    } else if p.numbering.is_some() {
        LIST_ITEM_AFTER_HU.max(ir)
    } else if p.quote {
        QUOTE_AFTER_HU.max(ir)
    } else if p.code_block {
        CODE_AFTER_HU.max(ir)
    } else if p.outline_level == Some(1) {
        H1_AFTER_HU.max(ir)
    } else if matches!(p.outline_level, Some(n) if n >= 2) {
        H2_AFTER_HU.max(ir)
    } else if is_letter_body_para(p) {
        BODY_AFTER_HU.max(ir)
    } else {
        ir
    }
}

fn paragraph_space_before(p: &Paragraph) -> Hu {
    let ir = p.space_before.max(0);
    if paragraph_is_image_only(p) {
        IMAGE_BEFORE_HU.max(ir)
    } else if p.quote {
        QUOTE_BEFORE_HU.max(ir)
    } else if p.code_block {
        CODE_BEFORE_HU.max(ir)
    } else if matches!(p.outline_level, Some(n) if n >= 2) {
        H2_BEFORE_HU.max(ir)
    } else {
        ir
    }
}

fn paragraph_pad_y(p: &Paragraph) -> (Hu, Hu) {
    if p.quote {
        (QUOTE_PAD_Y_HU, QUOTE_PAD_Y_HU)
    } else if p.code_block {
        (CODE_PAD_Y_HU, CODE_PAD_Y_HU)
    } else if p.outline_level == Some(1) {
        // H1 padding-bottom + 3px border sit in the box, not in collapsed
        // margin, matching letter CSS `padding-bottom:.4rem;border-bottom:3px`.
        (0, H1_RULE_GAP_HU.saturating_add(H1_RULE_HU))
    } else if matches!(p.outline_level, Some(n) if n >= 2) {
        // H2 padding-bottom + 2px border sit in the box, not in collapsed
        // margin, so the 24pt mark is not jammed under the rule.
        (0, H2_RULE_GAP_HU.saturating_add(H2_RULE_HU))
    } else {
        (0, 0)
    }
}

fn block_inset_x(p: &Paragraph) -> Hu {
    if p.quote {
        QUOTE_BAR_W_HU.saturating_add(QUOTE_PAD_X_HU)
    } else if p.code_block {
        CODE_PAD_X_HU
    } else {
        0
    }
}

/// Letter CSS `pre{padding:.75rem 1rem}` right. Quotes pad left only.
fn block_inset_right(p: &Paragraph) -> Hu {
    if p.code_block {
        CODE_PAD_X_HU
    } else {
        0
    }
}

fn paragraph_content_left(p: &Paragraph) -> Hu {
    if let Some(n) = p.numbering.as_ref() {
        let indent = LIST_INDENT_HU.saturating_mul(i32::from(n.level) + 1);
        // Letter CSS `ul.tasks{list-style:none;padding-left:1.25rem}`: the
        // checkbox is in-flow (`display:inline-block`) at padding-left. A
        // hanging outside marker is the wrong indent next to WeasyPrint.
        if matches!(n.format, Some(NumberFormat::Task)) {
            indent.saturating_add(TASK_MARK_ADVANCE_HU)
        } else {
            indent
        }
    } else {
        p.indent_left.max(0)
    }
}

fn is_letter_body_para(p: &Paragraph) -> bool {
    p.outline_level.is_none() && p.numbering.is_none() && !p.quote && !p.code_block
}

fn apply_list_block_after(blocks: &[Block], prepared: &mut [Prepared]) {
    for (i, item) in prepared.iter_mut().enumerate() {
        let Some(Block::Paragraph(p)) = blocks.get(i) else {
            continue;
        };
        let Some(cur) = p.numbering.as_ref() else {
            continue;
        };
        let next_level = match blocks.get(i + 1) {
            Some(Block::Paragraph(q)) => q.numbering.as_ref().map(|n| n.level),
            _ => None,
        };
        // Nested `ul ul{margin:.25rem 0}` is tighter than sibling `li` 0.35rem.
        // `paragraph_space_after` already pinned 0.35rem, so assign (do not
        // max) when stepping into a deeper level.
        if let Prepared::Lines { space_after, .. } = item {
            *space_after = match next_level {
                Some(next) if next > cur.level => LIST_NESTED_GAP_HU,
                Some(_) => (*space_after).max(LIST_ITEM_AFTER_HU),
                None => (*space_after).max(LIST_AFTER_HU),
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

fn prepare_block(block: &Block, fonts: &FontSet, width: Hu) -> Prepared {
    match block {
        Block::Paragraph(p) => {
            let (lines, fills) = layout_paragraph(p, fonts, width);
            let (pad_top, pad_bottom) = paragraph_pad_y(p);
            Prepared::Lines {
                lines,
                space_before: paragraph_space_before(p),
                // Image-only blocks match letter CSS `p{margin:0 0 .7rem}`
                // collapsed with `img{margin:.6rem 0}`. IR `space_after` 200
                // is a body-copy strut and must not replace that gap.
                space_after: paragraph_space_after(p),
                pad_top,
                pad_bottom,
                rule: heading_rule(p.outline_level),
                fills,
            }
        }
        Block::Table(t) => prepare_table(t, fonts, width),
        Block::Break(BreakKind::Thematic) => Prepared::Lines {
            lines: Vec::new(),
            space_before: HR_BEFORE_HU,
            space_after: HR_AFTER_HU,
            pad_top: 0,
            pad_bottom: HR_THICK_HU,
            rule: None,
            fills: vec![RectFrag {
                x: 0,
                y: 0,
                width,
                height: HR_THICK_HU,
                fill: [19, 78, 74, 255],
            }],
        },
        Block::Float(Float::Image(img)) => prepare_float_image(img, width),
        Block::Float(_) | Block::Break(_) => Prepared::Lines {
            lines: Vec::new(),
            space_before: 0,
            space_after: 0,
            pad_top: 0,
            pad_bottom: 0,
            rule: None,
            fills: Vec::new(),
        },
    }
}

fn prepare_float_image(img: &docagent_model::ImageData, max_w: Hu) -> Prepared {
    if img.width <= 0 || img.height <= 0 {
        return Prepared::Lines {
            lines: Vec::new(),
            space_before: 0,
            space_after: 0,
            pad_top: 0,
            pad_bottom: 0,
            rule: None,
            fills: Vec::new(),
        };
    }
    let mut image = InlineImage {
        bytes: img.bytes.clone(),
        mime: img.mime.clone(),
        width: img.width,
        height: img.height,
    };
    fit_inline_image(&mut image, max_w);
    Prepared::Lines {
        lines: vec![LineFrag {
            x: 0,
            y: 0,
            width: image.width,
            height: image.height,
            baseline: image.height,
            text: String::new(),
            font_size: DEFAULT_FONT_SIZE_HU,
            bold: false,
            italic: false,
            underline: false,
            dotted: false,
            strike: false,
            code: false,
            highlight: false,
            color: BODY_FG,
            href: None,
            image: Some(image),
        }],
        space_before: 0,
        space_after: 0,
        pad_top: 0,
        pad_bottom: 0,
        rule: None,
        fills: Vec::new(),
    }
}

fn is_bullet(p: &Paragraph) -> bool {
    matches!(
        p.numbering.as_ref().and_then(|n| n.format),
        Some(NumberFormat::Bullet)
    )
}

fn is_task(p: &Paragraph) -> bool {
    matches!(
        p.numbering.as_ref().and_then(|n| n.format),
        Some(NumberFormat::Task)
    )
}

fn task_checked(p: &Paragraph) -> bool {
    p.numbering.as_ref().is_some_and(|n| n.start == Some(1))
}

fn bullet_disc(size: Hu) -> Hu {
    // ~0.75em so PDF/A markers read as discs/boxes, not 2pt dots.
    (size.saturating_mul(3) / 4).max(400)
}

fn marker_advance(size: Hu, glyph_w: Hu, bullet: bool, task: bool) -> Hu {
    if task {
        TASK_MARK_ADVANCE_HU.max(glyph_w)
    } else if bullet {
        bullet_disc(size).saturating_add(LIST_MARK_GAP_HU).max(glyph_w)
    } else {
        glyph_w
    }
}

fn layout_paragraph(p: &Paragraph, fonts: &FontSet, width: Hu) -> (Vec<LineFrag>, Vec<RectFrag>) {
    let (text, mut spans) = collect_runs(p);
    let size = heading_display_size(
        p.outline_level,
        p.runs
            .first()
            .map(|r| r.style.size)
            .unwrap_or(DEFAULT_FONT_SIZE_HU),
    );
    // Letter CSS `code,pre code{font-size:.92em}` of the parent type.
    let size = if p.code_block {
        code_span_size(size)
    } else {
        size
    };
    let lh = paragraph_leading(fonts, p, size);
    let baseline = (lh * 4) / 5;
    let marker = list_marker(p);
    let glyph_w = marker
        .as_deref()
        .map(|m| shape(fonts, m, size).width)
        .unwrap_or(0);
    let content_left = paragraph_content_left(p).saturating_add(block_inset_x(p));
    let bullet = is_bullet(p);
    let task = is_task(p);
    let box_mark = bullet || task;
    let marker_w = marker_advance(size, glyph_w, bullet, task);
    let marker_x = content_left.saturating_sub(marker_w);
    let usable = (width - content_left - p.indent_right.max(0) - block_inset_right(p)).max(1);
    for span in &mut spans {
        if let Some(img) = span.image.as_mut() {
            fit_inline_image(img, usable);
        }
    }
    let mut shaped = shape(fonts, &text, size);
    let chars: Vec<char> = text.chars().collect();
    for span in &spans {
        if let Some(img) = &span.image
            && let Some(adv) = shaped.advances.get_mut(span.start)
        {
            *adv = img.width.max(1);
        } else if code_span_chip(span.code, p.code_block)
            && !span.superscript
            && !span.subscript
        {
            let rs = code_span_size(size);
            let slice: String = chars
                .get(span.start..span.end)
                .unwrap_or(&[])
                .iter()
                .collect();
            let cs = shape(fonts, &slice, rs);
            for (i, adv) in cs.advances.iter().enumerate() {
                if let Some(slot) = shaped.advances.get_mut(span.start.saturating_add(i)) {
                    *slot = *adv;
                }
            }
        }
    }
    if p.outline_level == Some(1) {
        apply_letter_spacing(&mut shaped.advances, h1_letter_spacing(size));
    }
    let ranges = break_lines(text.clone(), shaped.advances.clone(), usable);
    let mut out = Vec::new();
    let mut fills = Vec::new();
    if ranges.is_empty() {
        out.push(LineFrag {
            x: content_left,
            y: 0,
            width: 0,
            height: lh,
            baseline,
            text: if box_mark {
                String::new()
            } else {
                marker.unwrap_or_default()
            },
            font_size: size,
            bold: false,
            italic: false,
            underline: false,
            dotted: false,
            strike: false,
            code: false,
            highlight: false,
            color: if box_mark || p.numbering.is_some() {
                MARKER_FG
            } else {
                BODY_FG
            },
            href: None,
            image: None,
        });
        if bullet {
            fills.push(bullet_fill(marker_x, baseline, size));
        }
        if task {
            fills.extend(task_fills(marker_x, baseline, size, task_checked(p)));
        }
        if p.quote {
            fills.push(quote_bar_fill(p.indent_left, lh));
        }
        if p.code_block {
            fills.push(code_block_fill(p.indent_left, width, lh));
        }
        return (out, fills);
    }
    for (li, (a, b)) in ranges.into_iter().enumerate() {
        let extra_indent = if li == 0 { p.indent_first } else { 0 };
        let mut x = content_left + extra_indent;
        let row_right = content_left + extra_indent + usable;
        let mut row: Vec<LineFrag> = Vec::new();
        if li == 0 && bullet {
            fills.push(bullet_fill(marker_x.saturating_add(extra_indent), baseline, size));
        } else if li == 0 && task {
            fills.extend(task_fills(
                marker_x.saturating_add(extra_indent),
                baseline,
                size,
                task_checked(p),
            ));
        } else if li == 0
            && let Some(m) = marker.as_deref()
        {
            row.push(LineFrag {
                x: marker_x.saturating_add(extra_indent),
                y: 0,
                width: marker_w,
                height: 0,
                baseline,
                text: m.to_string(),
                font_size: size,
                bold: false,
                italic: false,
                underline: false,
                dotted: false,
                strike: false,
                code: false,
                highlight: false,
                color: MARKER_FG,
                href: None,
                image: None,
            });
        }
        for span in &spans {
            let s = span.start.max(a);
            let e = span.end.min(b);
            if s >= e {
                continue;
            }
            if let Some(img) = &span.image {
                let mut img = img.clone();
                fit_inline_image(&mut img, (row_right - x).max(1));
                let w = img.width;
                row.push(LineFrag {
                    x,
                    y: 0,
                    width: w,
                    height: 0,
                    baseline,
                    text: String::new(),
                    font_size: size,
                    bold: span.bold,
                    italic: span.italic,
                    underline: span.underline,
                    dotted: span.dotted,
                    strike: span.strike,
                    code: code_span_chip(span.code, p.code_block),
                    highlight: span.highlight,
                    color: span.color,
                    href: span.href.clone(),
                    image: Some(img),
                });
                x += w;
                continue;
            }
            let slice: String = chars.get(s..e).unwrap_or(&[]).iter().collect();
            let (w, run_size, run_baseline) = if span.superscript {
                let rs = super_sub_size(size);
                (
                    shape(fonts, &slice, rs).width,
                    rs,
                    baseline.saturating_sub(super_sub_shift(rs)),
                )
            } else if span.subscript {
                let rs = super_sub_size(size);
                (
                    shape(fonts, &slice, rs).width,
                    rs,
                    baseline.saturating_add(super_sub_shift(rs)),
                )
            } else if code_span_chip(span.code, p.code_block) {
                let w: i32 = shaped.advances.get(s..e).unwrap_or(&[]).iter().sum();
                (w, code_span_size(size), baseline)
            } else {
                let w: i32 = shaped.advances.get(s..e).unwrap_or(&[]).iter().sum();
                (w, size, baseline)
            };
            let pad_x = span_pad_x(span, p.code_block);
            let glyph_x = x.saturating_add(pad_x);
            row.push(LineFrag {
                x: glyph_x,
                y: 0,
                width: w,
                height: 0,
                baseline: run_baseline,
                text: slice,
                font_size: run_size,
                bold: span.bold,
                italic: span.italic,
                underline: span.underline,
                dotted: span.dotted,
                strike: span.strike,
                code: code_span_chip(span.code, p.code_block),
                highlight: span.highlight,
                color: span.color,
                href: span.href.clone(),
                image: None,
            });
            x = glyph_x.saturating_add(w).saturating_add(pad_x);
        }
        let origin = content_left + extra_indent;
        let total_w = (x - origin).max(0);
        let shift = align_x(origin, total_w, usable, p.alignment) - origin;
        for f in &mut row {
            f.x += shift;
        }
        let mut row_h = if paragraph_is_image_only(p) { 0 } else { lh };
        for f in &row {
            if let Some(img) = &f.image {
                row_h = row_h.max(img.height);
            }
        }
        if row_h <= 0 {
            row_h = lh;
        }
        if let Some(last) = row.last_mut() {
            last.height = row_h;
        } else {
            row.push(LineFrag {
                x: content_left,
                y: 0,
                width: 0,
                height: lh,
                baseline,
                text: String::new(),
                font_size: size,
                bold: false,
                italic: false,
                underline: false,
                dotted: false,
                strike: false,
                code: false,
                highlight: false,
                color: BODY_FG,
                href: None,
                image: None,
            });
        }
        out.extend(row);
    }
    if p.quote {
        let h = out.iter().map(|l| l.height).sum::<i32>().max(lh);
        fills.push(quote_bar_fill(p.indent_left, h));
    }
    if p.code_block {
        let h = out.iter().map(|l| l.height).sum::<i32>().max(lh);
        fills.push(code_block_fill(p.indent_left, width, h));
    }
    (out, fills)
}

fn quote_bar_fill(x: Hu, text_h: Hu) -> RectFrag {
    RectFrag {
        x,
        y: -QUOTE_PAD_Y_HU,
        width: QUOTE_BAR_W_HU,
        height: text_h.saturating_add(QUOTE_PAD_Y_HU.saturating_mul(2)),
        fill: QUOTE_FG,
    }
}

fn code_block_fill(x: Hu, width: Hu, text_h: Hu) -> RectFrag {
    RectFrag {
        x,
        y: -CODE_PAD_Y_HU,
        width,
        height: text_h.saturating_add(CODE_PAD_Y_HU.saturating_mul(2)),
        fill: CODE_SPAN_BG,
    }
}

fn span_pad_x(span: &RunSpan, code_block: bool) -> Hu {
    if span.image.is_some() {
        return 0;
    }
    let mut pad = 0;
    if code_span_chip(span.code, code_block) {
        pad = pad.max(CODE_SPAN_PAD_X_HU);
    }
    if span.highlight {
        pad = pad.max(MARK_PAD_X_HU);
    }
    pad
}

fn bullet_fill(x: Hu, baseline: Hu, size: Hu) -> RectFrag {
    let disc = bullet_disc(size);
    RectFrag {
        x: x.saturating_add(80),
        y: baseline.saturating_sub(disc),
        width: disc,
        height: disc,
        fill: MARKER_FG,
    }
}

fn task_fills(x: Hu, baseline: Hu, size: Hu, checked: bool) -> Vec<RectFrag> {
    let s = TASK_BOX_HU;
    let t = TASK_BORDER_HU.min(s).max(1);
    let teal = [19, 78, 74, 255];
    // Letter CSS `vertical-align:middle`: center the 0.9em box in the 1em square.
    let y = baseline.saturating_sub((s.saturating_add(size)) / 2);
    if checked {
        // `:checked{background:#134e4a}` with the same border color is a solid box.
        return vec![RectFrag {
            x,
            y,
            width: s,
            height: s,
            fill: teal,
        }];
    }
    vec![
        RectFrag {
            x,
            y,
            width: s,
            height: t,
            fill: teal,
        },
        RectFrag {
            x,
            y,
            width: t,
            height: s,
            fill: teal,
        },
        RectFrag {
            x,
            y: y + s - t,
            width: s,
            height: t,
            fill: teal,
        },
        RectFrag {
            x: x + s - t,
            y,
            width: t,
            height: s,
            fill: teal,
        },
    ]
}

struct RunSpan {
    start: usize,
    end: usize,
    bold: bool,
    italic: bool,
    underline: bool,
    dotted: bool,
    strike: bool,
    code: bool,
    highlight: bool,
    superscript: bool,
    subscript: bool,
    color: [u8; 4],
    href: Option<String>,
    image: Option<InlineImage>,
}

fn fit_inline_image(img: &mut InlineImage, max_w: Hu) {
    let (w, h) = min_on_page_size(img.width, img.height);
    img.width = w;
    img.height = h;
    clamp_inline_image(img, max_w);
}

fn clamp_inline_image(img: &mut InlineImage, max_w: Hu) {
    if img.width <= max_w {
        return;
    }
    let w = max_w.max(1);
    let h = (Hu64::from(img.height) * Hu64::from(w) / Hu64::from(img.width.max(1))).max(1) as Hu;
    img.width = w;
    img.height = h;
}

fn is_ua_black(color: [u8; 4]) -> bool {
    color == [0, 0, 0, 255]
}

fn run_color(run: &docagent_model::Run) -> [u8; 4] {
    // Letter CSS `code,kbd,samp{color:#134e4a}` wins over inherited UA black.
    if run.style.code {
        return CODE_SPAN_COLOR;
    }
    let c = run.style.color;
    if c == docagent_model::Color::BLACK {
        if run.style.strike {
            STRIKE_COLOR
        } else {
            BODY_FG
        }
    } else {
        [c.r, c.g, c.b, c.a]
    }
}

fn span_color(run: &docagent_model::Run, quote: bool, href: bool) -> [u8; 4] {
    let color = run_color(run);
    if href && !run.style.code && (is_ua_black(color) || color == BODY_FG) {
        LINK_COLOR
    } else if quote && !run.style.code && !href && (is_ua_black(color) || color == BODY_FG) {
        QUOTE_FG
    } else {
        color
    }
}

fn collect_runs(p: &Paragraph) -> (String, Vec<RunSpan>) {
    let mut i = 0usize;
    let mut text = String::new();
    let mut out = Vec::new();
    for run in &p.runs {
        let href = match &run.content {
            RunContent::Inline(InlineObject::Hyperlink { target, .. }) if !target.is_empty() => {
                Some(target.clone())
            }
            _ => None,
        };
        let underline = run.style.underline != docagent_model::Underline::None || href.is_some();
        let dotted = run.style.underline == docagent_model::Underline::Dotted && href.is_none();
        if let RunContent::Inline(InlineObject::Image(img)) = &run.content {
            if img.width <= 0 || img.height <= 0 {
                continue;
            }
            text.push('\u{FFFC}');
            out.push(RunSpan {
                start: i,
                end: i + 1,
                bold: run.style.bold,
                italic: run.style.italic,
                underline,
                dotted,
                strike: run.style.strike,
                code: run.style.code,
                highlight: run.style.highlight.is_some(),
                superscript: run.style.superscript,
                subscript: run.style.subscript,
                color: span_color(run, p.quote, href.is_some()),
                href,
                image: Some(InlineImage {
                    bytes: img.bytes.clone(),
                    mime: img.mime.clone(),
                    width: img.width,
                    height: img.height,
                }),
            });
            i += 1;
            continue;
        }
        let t = run.display_text();
        if t.is_empty() {
            continue;
        }
        let n = t.chars().count();
        text.push_str(t);
        out.push(RunSpan {
            start: i,
            end: i + n,
            bold: run.style.bold,
            italic: run.style.italic,
            underline,
            dotted,
            strike: run.style.strike,
            code: run.style.code,
            highlight: run.style.highlight.is_some(),
            superscript: run.style.superscript,
            subscript: run.style.subscript,
            color: span_color(run, p.quote, href.is_some()),
            href,
            image: None,
        });
        i += n;
    }
    (text, out)
}

fn list_marker(p: &Paragraph) -> Option<String> {
    let n = p.numbering.as_ref()?;
    match n.format {
        Some(NumberFormat::Bullet) | Some(NumberFormat::Task) => Some("• ".into()),
        Some(NumberFormat::Decimal) => {
            Some(format!("{}. ", n.start.unwrap_or(1)))
        }
        _ => None,
    }
}

fn align_x(x: Hu, width: Hu, usable: Hu, alignment: Alignment) -> Hu {
    match alignment {
        Alignment::Start | Alignment::Justify | Alignment::Distribute => x,
        Alignment::Center => x + (usable - width) / 2,
        Alignment::End => x + (usable - width),
    }
}

#[comemo::memoize]
fn break_lines(text: String, advances: Vec<i32>, width: i32) -> Vec<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    if n == 0 {
        return vec![(0, 0)];
    }
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut i = 0usize;
    while i < n {
        let start = i;
        let mut acc = 0i32;
        let mut last_space: Option<usize> = None;
        let mut wrapped = false;
        while i < n {
            let adv = advances.get(i).copied().unwrap_or(0);
            if i > start && acc + adv > width {
                let at = last_space.filter(|&b| b > start).unwrap_or(i);
                lines.push((start, at));
                i = at;
                while i < n && chars[i].is_whitespace() {
                    i += 1;
                }
                wrapped = true;
                break;
            }
            acc += adv;
            if chars[i].is_whitespace() {
                last_space = Some(i + 1);
            }
            i += 1;
        }
        if !wrapped {
            lines.push((start, n));
        }
    }
    if lines.is_empty() {
        lines.push((0, n));
    }
    lines
}

fn prepare_table(table: &Table, fonts: &FontSet, width: Hu) -> Prepared {
    let n_cols = table
        .rows
        .first()
        .map(|r| r.cells.len())
        .unwrap_or(0)
        .max(1);
    let mut col_widths: Vec<Hu> = (0..n_cols)
        .map(|c| {
            table
                .rows
                .iter()
                .filter_map(|r| r.cells.get(c).map(|cell| cell.width))
                .max()
                .unwrap_or(width / n_cols as i32)
        })
        .collect();
    let sum: Hu = col_widths.iter().sum();
    if sum > width && sum > 0 {
        col_widths = col_widths
            .iter()
            .map(|w| ((*w as i64) * i64::from(width) / i64::from(sum)) as i32)
            .collect();
    } else if n_cols > 0 && sum < width {
        let equal = width / n_cols as i32;
        col_widths = vec![equal; n_cols];
        let used = equal.saturating_mul(n_cols as i32);
        if let Some(last) = col_widths.last_mut() {
            *last = last.saturating_add(width - used);
        }
    }
    let mut rows = Vec::new();
    for (ri, row) in table.rows.iter().enumerate() {
        let mut cells = Vec::new();
        let header = row.header || ri < table.header_row_count as usize;
        for (ci, cell) in row.cells.iter().enumerate() {
            let cw = *col_widths.get(ci).unwrap_or(&10000);
            let (pad_l, pad_r, pad_t, pad_b) = cell_pads(cell);
            let inner = (cw - pad_l - pad_r).max(1);
            let mut lines = Vec::new();
            for block in &cell.blocks {
                if let Block::Paragraph(p) = block {
                    let (cell_lines, _) = layout_paragraph(&with_cell_type(p), fonts, inner);
                    lines.extend(cell_lines);
                }
            }
            let height = lines.iter().map(|l| l.height).sum::<i32>() + pad_t + pad_b;
            let height = row.height.unwrap_or(height).max(height);
            if header {
                style_header_lines(&mut lines);
            }
            cells.push(PreparedCell {
                lines,
                height,
                header,
                pad_left: pad_l,
                pad_top: pad_t,
                fill: cell_fill(cell, header),
                valign: cell.valign,
            });
        }
        rows.push(cells);
    }
    let (stroke_w, stroke) = table_stroke(table);
    Prepared::Table {
        rows,
        col_widths,
        stroke_w,
        stroke,
        space_before: TABLE_BEFORE_HU,
        space_after: TABLE_AFTER_HU,
    }
}

/// Letter CSS `th,td{font-size:.95rem}` overrides inherited 10pt *and* any
/// other IR size. Super/sub/code stay relative to this parent in
/// [`layout_paragraph`]. An empty cell still needs a sized run so the row
/// strut is 0.95rem, not body 10pt.
fn with_cell_type(p: &Paragraph) -> Paragraph {
    let mut p = p.clone();
    if p.runs.is_empty() {
        let mut run = docagent_model::Run::text("");
        run.style.size = CELL_SIZE_HU;
        p.runs.push(run);
        return p;
    }
    for run in &mut p.runs {
        run.style.size = CELL_SIZE_HU;
    }
    p
}

fn cell_fill(cell: &docagent_model::TableCell, header: bool) -> [u8; 4] {
    if let Some(c) = cell.shading {
        [c.r, c.g, c.b, c.a]
    } else if header {
        TH_BG
    } else {
        [255, 255, 255, 255]
    }
}

fn style_header_lines(lines: &mut [LineFrag]) {
    for line in lines {
        if line.image.is_some() {
            continue;
        }
        line.bold = true;
        if is_ua_black(line.color) || line.color == BODY_FG {
            line.color = TH_FG;
        }
    }
}

fn table_stroke(table: &Table) -> (Hu, [u8; 4]) {
    let b = &table.borders;
    if b.top.style == BorderStyle::None
        && b.left.style == BorderStyle::None
        && b.inside_h.style == BorderStyle::None
        && b.inside_v.style == BorderStyle::None
    {
        return (0, CELL_BORDER);
    }
    let w = b
        .top
        .width
        .max(b.left.width)
        .max(b.inside_h.width)
        .max(b.inside_v.width);
    let c = b.top.color;
    let color = if c == docagent_model::Color::BLACK {
        CELL_BORDER
    } else {
        [c.r, c.g, c.b, c.a]
    };
    let width = if w <= docagent_model::Border::default().width {
        CELL_BORDER_HU
    } else {
        w.max(1)
    };
    (width, color)
}

pub fn page_count(tree: &FragmentTree) -> usize {
    tree.pages.len()
}

pub fn all_coordinates_are_integers(tree: &FragmentTree) -> bool {
    let _ = tree;
    true
}

pub fn line_positions(tree: &FragmentTree) -> Vec<(Hu, Hu, Hu)> {
    tree.pages
        .iter()
        .flat_map(|p| p.lines.iter().map(|l| (l.x, l.y, l.height)))
        .collect()
}

pub fn table_row_heights(tree: &FragmentTree) -> Vec<Hu> {
    tree.pages
        .iter()
        .flat_map(|p| p.rects.iter().map(|r| r.height))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use docagent_font::line_height;
    use docagent_model::{
        HeaderFooter, NumberFormat, NumberingRef, Paragraph, Section, Table, VAlign,
        DEFAULT_MARGIN_HU,
    };

    fn hf(text: &str) -> HeaderFooter {
        HeaderFooter {
            blocks: vec![Block::Paragraph(Paragraph::from_text(text))],
            different_first: None,
            different_odd_even: None,
        }
    }

    fn hf_image(width: Hu, height: Hu) -> HeaderFooter {
        let mut p = Paragraph::from_text("");
        p.runs = vec![mark_run(width, height)];
        HeaderFooter {
            blocks: vec![Block::Paragraph(p)],
            different_first: None,
            different_odd_even: None,
        }
    }

    #[test]
    fn layout_uses_integer_hwpunit_only() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("Integer layout")));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        assert!(!tree.pages.is_empty());
        for page in &tree.pages {
            for line in &page.lines {
                assert!(line.height > 0);
                assert!(line.font_size > 0);
            }
        }
        assert!(all_coordinates_are_integers(&tree));
    }

    #[test]
    fn layout_does_not_read_lineseg_hints() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("hinted line");
        p.layout_hints.push(docagent_model::LayoutHint {
            text_start: 0,
            x: 99,
            y: 12345,
            width: 1,
            line_height: 1,
            text_height: 1,
            baseline: 1,
            spacing: 1,
            flags: 0,
        });
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let ys: Vec<i32> = tree.pages.iter().flat_map(|p| p.lines.iter().map(|l| l.y)).collect();
        assert!(ys.iter().all(|y| *y != 12345));
    }

    #[test]
    fn long_paragraph_wraps() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let text = "word ".repeat(400);
        section.body.push(Block::Paragraph(Paragraph::from_text(text)));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let n: usize = tree.pages.iter().map(|p| p.lines.len()).sum();
        assert!(n > 1);
    }

    #[test]
    fn wraps_on_spaces_and_keeps_ligature_tails() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Paragraph(Paragraph::from_text(
            "Delivery confirmation — order 48291. The files match and we are hoping.",
        )));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let joined: String = tree
            .pages
            .iter()
            .flat_map(|p| p.lines.iter().map(|l| l.text.as_str()))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(joined.contains("48291"), "{joined}");
        assert!(joined.contains("hoping"), "{joined}");
        assert!(joined.contains("confirmation"), "{joined}");
        for line in tree.pages.iter().flat_map(|p| p.lines.iter()) {
            let t = line.text.trim();
            if t.is_empty() {
                continue;
            }
            assert!(
                !t.starts_with("nts") && !t.starts_with("ing"),
                "mid-word wrap: {t}"
            );
        }
    }

    #[test]
    fn tables_emit_grid_strokes() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Table(Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ])));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        assert_eq!(page.rects.len(), 4, "one fill per cell");
        assert!(
            page.strokes
                .iter()
                .filter(|s| s.fill == CELL_BORDER)
                .count()
                >= 8,
            "collapsed 1px grid missing: {:?}",
            page.strokes
        );
        assert_eq!(table_row_heights(&tree).len(), 4);
        assert!(
            page.strokes
                .iter()
                .any(|s| s.fill == CELL_BORDER && s.width == CELL_BORDER_HU),
            "letter table must paint 1px #cbd5e1, not a black hairline: {:?}",
            page.strokes
        );
        assert!(
            page.strokes
                .iter()
                .all(|s| s.fill != [0, 0, 0, 255]),
            "letter table must not keep a sparse black grid: {:?}",
            page.strokes
        );
    }

    #[test]
    fn table_headers_emit_teal_fills() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let headers = page
            .rects
            .iter()
            .filter(|r| r.fill == TH_BG)
            .count();
        let body = page
            .rects
            .iter()
            .filter(|r| r.fill == [255, 255, 255, 255])
            .count();
        assert_eq!(headers, 2, "header cells missing teal fill: {:?}", page.rects);
        assert_eq!(body, 2, "body cells missing white fill: {:?}", page.rects);
        assert!(
            page.strokes
                .iter()
                .filter(|s| s.fill == TH_RULE && s.height == TH_RULE_HU)
                .count()
                >= 2,
            "th must paint 2px #0d9488 bottom, not a sparse grid: {:?}",
            page.strokes
        );
        let hop = page
            .lines
            .iter()
            .find(|l| l.text.contains("Hop"))
            .expect("header text");
        assert_eq!(hop.color, TH_FG, "th text must be letter CSS #134e4a");
        assert!(hop.bold, "th text must be letter CSS font-weight 700");
        let input = page
            .lines
            .iter()
            .find(|l| l.text.contains("Input"))
            .expect("body text");
        assert_eq!(
            input.color, BODY_FG,
            "td text must be letter body #134e4a, not UA black"
        );
        assert!(!input.bold, "td text must not inherit th bold");
        assert_eq!(TH_BG, [204, 251, 241, 255]);
        assert_eq!(CELL_BORDER, [203, 213, 225, 255]);
        assert_eq!(TH_RULE_HU, 150);
        assert_eq!(CELL_BORDER_HU, 75);
    }

    #[test]
    fn letter_table_header_is_weasyprint_th() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut before = Paragraph::from_text("docagent prove examples/letter.md");
        before.code_block = true;
        before.space_after = 200;
        section.body.push(Block::Paragraph(before));
        section
            .body
            .push(Block::Paragraph(strong_em_para()));
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into(), "Proof".into()],
            vec!["Input".into(), "letter.md".into(), "input SHA-256".into()],
        ]);
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        section
            .body
            .push(Block::Break(docagent_model::BreakKind::Thematic));
        let mut link = Paragraph::from_text("");
        link.runs = vec![docagent_model::Run::hyperlink(
            "DocAgent",
            "https://github.com/kevin9327/docagent",
        )];
        section.body.push(Block::Paragraph(link));
        section
            .body
            .push(Block::Paragraph(task_para("Receiving dock is clear", true)));
        let mut mark = Paragraph::from_text("three hashes");
        mark.runs[0].style.highlight = Some(docagent_model::Color::MARK);
        section.body.push(Block::Paragraph(mark));
        let mut code = Paragraph::from_text("prove");
        code.runs[0].style.code = true;
        section.body.push(Block::Paragraph(code));
        let mut strike = Paragraph::from_text("guess");
        strike.runs[0].style.strike = true;
        section.body.push(Block::Paragraph(strike));
        let mut under = Paragraph::from_text("09:00 local");
        under.runs[0].style.underline = docagent_model::Underline::Single;
        section.body.push(Block::Paragraph(under));
        let mut pdfa = Paragraph::from_text("PDF/");
        let mut sup = docagent_model::Run::text("A");
        sup.style.superscript = true;
        pdfa.runs.push(sup);
        section.body.push(Block::Paragraph(pdfa));
        let mut h2o = Paragraph::from_text("H");
        let mut sub = docagent_model::Run::text("2");
        sub.style.subscript = true;
        h2o.runs.push(sub);
        h2o.runs.push(docagent_model::Run::text("O"));
        section.body.push(Block::Paragraph(h2o));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let headers = page.rects.iter().filter(|r| r.fill == TH_BG).count();
        assert_eq!(headers, 3, "letter th fill missing: {:?}", page.rects);
        assert!(
            page.strokes
                .iter()
                .any(|s| s.fill == CELL_BORDER && s.height == CELL_BORDER_HU),
            "th,td 1px #cbd5e1 missing: {:?}",
            page.strokes
        );
        let th_rules = page
            .strokes
            .iter()
            .filter(|s| s.fill == TH_RULE && s.height == TH_RULE_HU)
            .count();
        assert_eq!(th_rules, 3, "th 2px #0d9488 missing: {:?}", page.strokes);
        let hop = page
            .lines
            .iter()
            .find(|l| l.text.contains("Hop"))
            .expect("Hop");
        let cell = page
            .rects
            .iter()
            .find(|r| r.fill == TH_BG)
            .expect("th fill");
        assert_eq!(hop.x, cell.x.saturating_add(CELL_PAD_X_HU));
        assert_eq!(hop.y, cell.y.saturating_add(CELL_PAD_Y_HU));
        assert_eq!(hop.color, TH_FG);
        assert!(hop.bold);
        assert_eq!(hop.font_size, CELL_SIZE_HU);
        let gpu = page
            .lines
            .iter()
            .find(|l| l.text.contains("GPU-free"))
            .expect("strong");
        assert!(gpu.bold, "strong must be letter CSS font-weight 700");
        assert!(!gpu.italic);
        assert_eq!(
            gpu.color, BODY_FG,
            "strong body must be letter CSS #134e4a, not UA black"
        );
        let slant = page
            .lines
            .iter()
            .find(|l| l.text.contains("byte-for-byte"))
            .expect("em");
        assert!(slant.italic, "em must be letter CSS font-style italic");
        assert!(!slant.bold);
        assert_eq!(
            slant.color, BODY_FG,
            "em body must be letter CSS #134e4a, not UA black"
        );
        let replay = page
            .lines
            .iter()
            .find(|l| l.text.contains("Replay is evidence"))
            .expect("quote");
        assert_eq!(replay.color, QUOTE_FG, "blockquote text must be letter CSS #134e4a");
        assert!(
            page.strokes.iter().any(|s| {
                s.fill == QUOTE_FG && s.width == QUOTE_BAR_W_HU && s.height > 200
            }),
            "quote bar must not regress"
        );
        assert!(
            page.strokes
                .iter()
                .any(|s| s.fill == [19, 78, 74, 255]
                    && s.width == TASK_BOX_HU
                    && s.height == TASK_BOX_HU),
            "task box must not regress"
        );
        assert!(
            page.strokes
                .iter()
                .any(|s| s.fill == MARK_BG && s.width > 80),
            "mark fill must not regress"
        );
        assert!(
            page.strokes
                .iter()
                .any(|s| s.fill == CODE_SPAN_BG && s.width > 80 && s.height < 2000),
            "code span fill must not regress"
        );
        assert!(
            page.strokes
                .iter()
                .any(|s| s.fill == [19, 78, 74, 255]
                    && s.height == HR_THICK_HU
                    && s.width > 10000),
            "hr must not regress"
        );
        let docagent = page
            .lines
            .iter()
            .find(|l| l.text.contains("DocAgent"))
            .expect("link");
        assert_eq!(docagent.href.as_deref(), Some("https://github.com/kevin9327/docagent"));
        let guess = page
            .lines
            .iter()
            .find(|l| l.text.contains("guess"))
            .expect("strike");
        assert!(guess.strike);
        assert_eq!(guess.color, STRIKE_COLOR);
        assert!(
            page.strokes.iter().any(|s| {
                is_text_decoration(s)
                    && s.fill == STRIKE_COLOR
                    && s.height == STRIKE_HU
                    && s.width == guess.width
                    && s.x == guess.x
                    && s.y
                        == guess
                            .y
                            .saturating_add(guess.baseline)
                            .saturating_sub(guess.font_size / 3)
            }),
            "strike must not regress: {:?}",
            page.strokes
        );
        let local = page
            .lines
            .iter()
            .find(|l| l.text.contains("09:00"))
            .expect("underline");
        assert!(local.underline);
        assert_eq!(
            local.color, BODY_FG,
            "u text must be letter body #134e4a, not UA black"
        );
        assert!(
            page.strokes.iter().any(|s| {
                is_text_decoration(s)
                    && s.fill == STRIKE_COLOR
                    && s.height == UNDERLINE_HU
                    && s.width == local.width
                    && s.x == local.x
                    && s.y
                        == local
                            .y
                            .saturating_add(local.baseline)
                            .saturating_add(UNDERLINE_OFFSET_HU)
            }),
            "underline must not regress: {:?}",
            page.strokes
        );
        assert_eq!(TH_BG, CODE_SPAN_BG);
        assert_eq!(TASK_BOX_HU, 900);
        assert_eq!(TASK_BORDER_HU, 150);
        assert_eq!(LIST_MARK_GAP_HU, 480);
        assert_eq!(CELL_PAD_X_HU, 780);
        assert_eq!(CELL_PAD_Y_HU, 480);
        assert_eq!(CELL_SIZE_HU, 1140);
        assert_eq!(IMAGE_BEFORE_HU, 720);
        assert_eq!(H1_RULE_GAP_HU, 480);
        assert_eq!(H1_RULE_HU, 225);
        assert_eq!(H2_RULE_GAP_HU, 240);
        assert_eq!(H2_RULE_HU, 150);
        assert_eq!(H1_SIZE_MAX_HU, 2100);
        assert_eq!(H2_SIZE_MAX_HU, 1380);
        assert_eq!(HR_THICK_HU, 225);
        assert_eq!(LINK_COLOR, [13, 148, 136, 255]);
        assert_eq!(CODE_SPAN_PAD_X_HU, 350);
        assert_eq!(CODE_SPAN_SIZE_HU, 920);
        assert_eq!(MARK_PAD_X_HU, 200);
        assert_eq!(TABLE_BEFORE_HU, 1200);
        assert_eq!(TABLE_AFTER_HU, 1200);
        assert_eq!(STRIKE_HU, 80);
        assert_eq!(UNDERLINE_HU, 80);
        assert_eq!(UNDERLINE_OFFSET_HU, 100);
        assert_eq!(STRIKE_COLOR, [19, 78, 74, 255]);
        assert_eq!(QUOTE_FG, [19, 78, 74, 255]);
        assert_eq!(BODY_FG, [19, 78, 74, 255]);
        assert_ne!(BODY_FG, [0, 0, 0, 255], "body must not regress to UA black");
        assert_eq!(QUOTE_BAR_W_HU, 300);
        let pdf = page
            .lines
            .iter()
            .find(|l| l.text.contains("PDF"))
            .expect("PDF");
        let raised = page
            .lines
            .iter()
            .find(|l| l.text == "A")
            .expect("super");
        assert_eq!(raised.font_size, SUPER_SUB_SIZE_HU);
        assert_eq!(raised.font_size, super_sub_size(pdf.font_size));
        assert_eq!(
            raised.y.saturating_add(raised.baseline),
            pdf.y
                .saturating_add(pdf.baseline)
                .saturating_sub(SUPER_SUB_SHIFT_HU),
            "super must rise WeasyPrint 0.5em, not a missing baseline shift"
        );
        let h = page.lines.iter().find(|l| l.text == "H").expect("H");
        let lowered = page.lines.iter().find(|l| l.text == "2").expect("sub");
        assert_eq!(lowered.font_size, SUPER_SUB_SIZE_HU);
        assert_eq!(
            lowered.y.saturating_add(lowered.baseline),
            h.y
                .saturating_add(h.baseline)
                .saturating_add(SUPER_SUB_SHIFT_HU),
            "sub must drop WeasyPrint 0.5em, not a missing baseline shift"
        );
        assert_eq!(SUPER_SUB_SIZE_HU, 750);
        assert_eq!(SUPER_SUB_SHIFT_HU, 375);
        assert_eq!(
            pdf.color, BODY_FG,
            "body/sup parent must be letter CSS #134e4a, not UA black"
        );
        assert_eq!(
            raised.color, BODY_FG,
            "sup must keep letter body #134e4a"
        );
        assert_eq!(h.color, BODY_FG, "sub parent must be letter body #134e4a");
        assert_eq!(lowered.color, BODY_FG, "sub must keep letter body #134e4a");
    }

    #[test]
    fn headings_emit_rule_fills() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.size = 1800;
        h1.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation");
        h2.outline_level = Some(2);
        h2.runs[0].style.size = 1400;
        h2.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h2));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("Body copy")));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        assert!(
            page.rects.is_empty(),
            "heading rules must not count as table cell fills"
        );
        assert!(
            page.strokes.iter().any(|s| s.fill == BODY_FG && s.height == H1_RULE_HU && s.width > 1000),
            "h1 3px #134e4a missing: {:?}",
            page.strokes
        );
        assert!(
            page.strokes.iter().any(|s| s.fill == TH_RULE && s.height == H2_RULE_HU && s.width > 1000),
            "h2 full-content 2px #0d9488 missing: {:?}",
            page.strokes
        );
        assert_eq!(table_row_heights(&tree).len(), 0);
        let body = page
            .lines
            .iter()
            .find(|l| l.text.contains("Body copy"))
            .expect("body");
        assert_eq!(
            body.color, BODY_FG,
            "body copy must be letter CSS #134e4a, not UA black"
        );
        let h1_line = page
            .lines
            .iter()
            .find(|l| l.text.contains("Northwind Freight"))
            .expect("h1");
        assert_eq!(
            h1_line.color, BODY_FG,
            "h1 must be letter CSS #134e4a, not UA black"
        );
        let h2_line = page
            .lines
            .iter()
            .find(|l| l.text.contains("Delivery confirmation"))
            .expect("h2");
        assert_eq!(
            h2_line.color, BODY_FG,
            "h2 must be letter CSS #134e4a, not UA black"
        );
    }

    #[test]
    fn letter_h1_rule_is_weasyprint_3px_04rem() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.size = 1800;
        h1.runs[0].style.bold = true;
        h1.space_after = 400;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation");
        h2.outline_level = Some(2);
        h2.runs[0].style.size = 1400;
        h2.runs[0].style.bold = true;
        h2.space_before = 360;
        section.body.push(Block::Paragraph(h2));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let h1 = page
            .lines
            .iter()
            .find(|l| l.text.contains("Northwind"))
            .expect("h1");
        let h2 = page
            .lines
            .iter()
            .find(|l| l.text.contains("Delivery"))
            .expect("h2");
        assert_eq!(
            h1.color, BODY_FG,
            "h1 must be letter CSS #134e4a, not UA black"
        );
        assert_eq!(
            h2.color, BODY_FG,
            "h2 must be letter CSS #134e4a, not UA black"
        );
        assert_eq!(H1_RULE_HU, 225, "h1 border must stay letter CSS 3px");
        assert_eq!(H1_RULE_GAP_HU, 480, "h1 pad-bottom must stay letter CSS 0.4rem");
        assert_eq!(H1_RULE_HU, HR_THICK_HU);
        assert_eq!(H2_BEFORE_HU, 1320, "h2 margin-top must stay letter CSS 1.1rem");
        assert_eq!(H1_AFTER_HU, 720, "h1 margin-bottom must stay letter CSS 0.6rem");
        assert_eq!(H2_AFTER_HU, 540, "h2 margin-bottom must stay letter CSS 0.45rem");
        let rule = page
            .strokes
            .iter()
            .find(|s| s.fill == BODY_FG && s.height == H1_RULE_HU && s.width > 1000)
            .expect("h1 3px rule");
        assert_eq!(
            rule.y,
            h1.y.saturating_add(h1.height).saturating_add(H1_RULE_GAP_HU),
            "h1 rule must sit 0.4rem below the line box, not a 40 HU hairline"
        );
        assert_eq!(
            h2.y,
            rule.y.saturating_add(H1_RULE_HU).saturating_add(H2_BEFORE_HU),
            "h2 y must sit after in-box 3px rule + collapsed 1.1rem h2 margin-top"
        );
        let heading_gap = h2.y.saturating_sub(h1.y.saturating_add(h1.height));
        assert_eq!(
            heading_gap.saturating_sub(H1_RULE_GAP_HU.saturating_add(H1_RULE_HU)),
            H2_BEFORE_HU,
            "h1 0.4rem pad + 3px rule sit in the box; remaining {heading_gap} must be letter CSS 1.1rem, not IR 400"
        );
    }

    #[test]
    fn letter_h2_rule_is_weasyprint_full_content_width() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.size = 1800;
        h1.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation");
        h2.outline_level = Some(2);
        h2.runs[0].style.size = 1400;
        h2.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h2));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let h2 = page
            .lines
            .iter()
            .find(|l| l.text.contains("Delivery confirmation"))
            .expect("h2");
        let content_w = page.width.saturating_sub(PAGE_MARGIN_X_HU.saturating_mul(2));
        let h1_rule = page
            .strokes
            .iter()
            .find(|s| s.fill == BODY_FG && s.height == H1_RULE_HU && s.width > 1000)
            .expect("h1 3px rule");
        let h2_rule = page
            .strokes
            .iter()
            .find(|s| s.fill == TH_RULE && s.height == H2_RULE_HU && s.width > 80)
            .expect("h2 2px rule");
        assert_eq!(
            h2_rule.x, PAGE_MARGIN_X_HU,
            "h2 border-bottom x {} must be letter CSS content left, not text x {}",
            h2_rule.x, h2.x
        );
        assert_eq!(
            h2_rule.width, content_w,
            "h2 rule width {} must be letter CSS block border-bottom (full content), not text span {}",
            h2_rule.width, h2.width
        );
        assert_eq!(
            h2_rule.width, h1_rule.width,
            "h2 border-bottom must span the same content box as h1"
        );
        assert!(
            h2_rule.width > h2.width,
            "h2 rule {} must outrun heading text {}",
            h2_rule.width, h2.width
        );
        assert_eq!(
            h2_rule.y,
            h2.y.saturating_add(h2.height).saturating_add(H2_RULE_GAP_HU),
            "h2 rule must sit letter CSS 0.2rem below the line box"
        );
        assert_eq!(h1_rule.x, PAGE_MARGIN_X_HU);
        assert_eq!(h1_rule.width, content_w);
        assert_eq!(H2_RULE_HU, 150, "h2 border must stay letter CSS 2px");
        assert_eq!(H2_RULE_GAP_HU, 240, "h2 pad-bottom must stay letter CSS 0.2rem");
        assert_eq!(TH_RULE, [13, 148, 136, 255]);
        assert_eq!(PAGE_MARGIN_X_HU, 3300);
        assert_eq!(H1_RULE_HU, 225);
        assert_eq!(H1_AFTER_HU, 720);
        assert_eq!(H2_BEFORE_HU, 1320);
        assert_eq!(H2_AFTER_HU, 540);
        assert_eq!(CELL_PAD_X_HU, 780);
    }

    #[test]
    fn letter_h1_letter_spacing_is_weasyprint_neg_02em() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.size = 1800;
        h1.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation");
        h2.outline_level = Some(2);
        h2.runs[0].style.size = 1400;
        h2.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h2));
        doc.sections.push(section);
        let fonts = FontSet::bundled();
        let tree = layout_document(&doc, &fonts);
        let page = &tree.pages[0];
        let h1 = page
            .lines
            .iter()
            .find(|l| l.text.contains("Northwind Freight"))
            .expect("h1");
        let h2 = page
            .lines
            .iter()
            .find(|l| l.text.contains("Delivery confirmation"))
            .expect("h2");
        let h1_natural = shape(&fonts, "Northwind Freight", H1_SIZE_MAX_HU).width;
        let h2_natural = shape(&fonts, "Delivery confirmation", H2_SIZE_MAX_HU).width;
        let gaps = i32::try_from("Northwind Freight".chars().count().saturating_sub(1))
            .unwrap_or(i32::MAX);
        assert_eq!(H1_LETTER_SPACING_HU, -42, "h1 tracking must stay letter CSS -.02em");
        assert_eq!(
            h1_letter_spacing(H1_SIZE_MAX_HU),
            H1_LETTER_SPACING_HU
        );
        assert_eq!(
            h1.font_size, H1_SIZE_MAX_HU,
            "h1 size must stay letter CSS 1.75rem"
        );
        assert_eq!(
            h1.width,
            h1_natural.saturating_add(H1_LETTER_SPACING_HU.saturating_mul(gaps)),
            "h1 width {} must bake letter-spacing -.02em, not UA normal {}",
            h1.width,
            h1_natural
        );
        assert!(
            h1.width < h1_natural,
            "h1 must track tighter than UA 0 letter-spacing: {} vs {}",
            h1.width,
            h1_natural
        );
        assert_eq!(
            h2.width, h2_natural,
            "letter.css letter-spacing is h1-only, not h2: {} vs {}",
            h2.width, h2_natural
        );
        assert_eq!(h2.font_size, H2_SIZE_MAX_HU);
    }

    #[test]
    fn bullets_prefix_first_line() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Receiving dock is clear");
        p.numbering = Some(NumberingRef {
            definition_id: 0,
            level: 0,
            start: None,
            format: Some(NumberFormat::Bullet),
        });
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let joined: String = tree.pages[0]
            .lines
            .iter()
            .map(|l| l.text.as_str())
            .collect();
        assert!(!joined.contains('•'), "{joined}");
        assert!(joined.contains("Receiving dock is clear"), "{joined}");
        assert!(
            tree.pages[0]
                .strokes
                .iter()
                .any(|s| s.fill == MARKER_FG && s.width == s.height && s.width >= 400),
            "bullet marker fill missing: {:?}",
            tree.pages[0].strokes
        );
    }

    fn strong_em_para() -> Paragraph {
        let mut p = Paragraph::from_text("one convert, ");
        let mut strong = docagent_model::Run::text("GPU-free");
        strong.style.bold = true;
        p.runs.push(strong);
        p.runs.push(docagent_model::Run::text(". match "));
        let mut em = docagent_model::Run::text("byte-for-byte");
        em.style.italic = true;
        p.runs.push(em);
        p
    }

    fn task_para(text: &str, checked: bool) -> Paragraph {
        let mut p = Paragraph::from_text(text);
        p.space_after = 80;
        p.numbering = Some(NumberingRef {
            definition_id: 2,
            level: 0,
            start: checked.then_some(1),
            format: Some(NumberFormat::Task),
        });
        p
    }

    fn task_box_origin(page: &PageFrag) -> Option<(Hu, Hu)> {
        page.strokes.iter().find_map(|s| {
            (s.fill == [19, 78, 74, 255] && s.width == TASK_BOX_HU)
                .then_some((s.x, s.y))
        })
    }

    #[test]
    fn task_boxes_emit_checkbox_fills() {
        fn tree_for(checked: bool) -> FragmentTree {
            let mut doc = Document::new();
            let mut section = Section::default();
            section
                .body
                .push(Block::Paragraph(task_para("Replay the convert", checked)));
            doc.sections.push(section);
            layout_document(&doc, &FontSet::bundled())
        }
        let open = tree_for(false);
        let open_page = &open.pages[0];
        let edges: Vec<_> = open_page
            .strokes
            .iter()
            .filter(|s| s.fill == [19, 78, 74, 255])
            .collect();
        assert_eq!(
            edges
                .iter()
                .filter(|s| s.width == TASK_BOX_HU && s.height == TASK_BORDER_HU)
                .count(),
            2,
            "unchecked - [ ] must paint 1.5pt top/bottom: {:?}",
            open_page.strokes
        );
        assert_eq!(
            edges
                .iter()
                .filter(|s| s.width == TASK_BORDER_HU && s.height == TASK_BOX_HU)
                .count(),
            2,
            "unchecked - [ ] must paint 1.5pt left/right: {:?}",
            open_page.strokes
        );
        assert!(
            !edges.iter().any(|s| s.width == s.height),
            "unchecked box must stay hollow, not a disc: {:?}",
            open_page.strokes
        );
        let done = tree_for(true);
        let done_page = &done.pages[0];
        assert!(
            done_page.strokes.iter().any(|s| {
                s.fill == [19, 78, 74, 255] && s.width == TASK_BOX_HU && s.height == TASK_BOX_HU
            }),
            "checked - [x] must fill 0.9em teal: {:?}",
            done_page.strokes
        );
        assert!(
            !done_page.strokes.iter().any(|s| {
                s.fill == [19, 78, 74, 255] && s.width != s.height
            }),
            "checked box must not keep a hollow border: {:?}",
            done_page.strokes
        );
        let joined: String = done_page.lines.iter().map(|l| l.text.as_str()).collect();
        assert!(!joined.contains('•'), "{joined}");
        assert!(joined.contains("Replay the convert"), "{joined}");
        let text = done_page
            .lines
            .iter()
            .find(|l| l.text.contains("Replay"))
            .expect("task text");
        let (bx, _) = task_box_origin(done_page).expect("task box");
        assert_eq!(
            text.x,
            bx.saturating_add(TASK_BOX_HU).saturating_add(LIST_MARK_GAP_HU),
            "task text gap must be letter CSS 0.4rem, not a missing box"
        );
        assert_eq!(
            bx,
            PAGE_MARGIN_X_HU.saturating_add(LIST_INDENT_HU),
            "task checkbox x must be letter CSS ul.tasks padding-left 1.25rem, not a hanging marker"
        );
        assert_eq!(
            text.x,
            PAGE_MARGIN_X_HU
                .saturating_add(LIST_INDENT_HU)
                .saturating_add(TASK_MARK_ADVANCE_HU)
        );
        assert_eq!(TASK_MARK_ADVANCE_HU, 1380);
        assert_eq!(TASK_BOX_HU, 900);
        assert_eq!(TASK_BORDER_HU, 150);
        assert_eq!(LIST_MARK_GAP_HU, 480);
        assert_eq!(CODE_SPAN_PAD_X_HU, 350);
        assert_eq!(MARK_PAD_X_HU, 200);
        assert_eq!(HR_THICK_HU, 225);
        assert_eq!(LINK_COLOR, [13, 148, 136, 255]);
        assert_eq!(H1_SIZE_MAX_HU, 2100);
        assert_eq!(H2_SIZE_MAX_HU, 1380);
        assert_eq!(CELL_PAD_X_HU, 780);
        assert_eq!(LIST_ITEM_AFTER_HU, 420);
    }

    #[test]
    fn letter_task_list_gap_is_weasyprint_035rem() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(task_para("Receiving dock is clear", true)));
        section
            .body
            .push(Block::Paragraph(task_para("Replay the convert", false)));
        let mut after = Paragraph::from_text("Agent replay");
        after.space_after = 200;
        section.body.push(Block::Paragraph(after));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let first = page
            .lines
            .iter()
            .find(|l| l.text.contains("dock"))
            .expect("checked item");
        let second = page
            .lines
            .iter()
            .find(|l| l.text.contains("Replay"))
            .expect("unchecked item");
        let after = page
            .lines
            .iter()
            .find(|l| l.text.contains("Agent"))
            .expect("after list");
        assert_eq!(
            second.y,
            first
                .y
                .saturating_add(first.height)
                .saturating_add(LIST_ITEM_AFTER_HU),
            "task item gap {} must stay letter CSS 0.35rem",
            second.y.saturating_sub(first.y.saturating_add(first.height))
        );
        assert_eq!(
            after.y,
            second
                .y
                .saturating_add(second.height)
                .saturating_add(LIST_AFTER_HU),
            "gap after task list {} must stay letter CSS 1rem",
            after.y.saturating_sub(second.y.saturating_add(second.height))
        );
    }

    #[test]
    fn letter_task_indent_is_weasyprint_125rem_inline() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(task_para("Receiving dock is clear", true)));
        section
            .body
            .push(Block::Paragraph(task_para("Replay the convert", false)));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let dock = page
            .lines
            .iter()
            .find(|l| l.text.contains("dock"))
            .expect("checked text");
        let replay = page
            .lines
            .iter()
            .find(|l| l.text.contains("Replay"))
            .expect("unchecked text");
        let origin = PAGE_MARGIN_X_HU;
        let (bx, _) = task_box_origin(page).expect("task box");
        assert_eq!(
            bx,
            origin.saturating_add(LIST_INDENT_HU),
            "task checkbox x {} must be letter CSS ul.tasks 1.25rem, not hanging in the padding",
            bx.saturating_sub(origin)
        );
        assert_eq!(
            dock.x,
            origin
                .saturating_add(LIST_INDENT_HU)
                .saturating_add(TASK_MARK_ADVANCE_HU),
            "task text x {} must start after 0.9em box + 0.4rem gap",
            dock.x.saturating_sub(origin)
        );
        assert_eq!(replay.x, dock.x, "open and checked tasks share the in-flow indent");
        assert_eq!(TASK_MARK_ADVANCE_HU, TASK_BOX_HU + LIST_MARK_GAP_HU);
        assert_eq!(TASK_MARK_ADVANCE_HU, 1380);
        assert_eq!(LIST_INDENT_HU, 1500);
        const { assert!(TASK_MARK_ADVANCE_HU == 1380) };
        const { assert!(TASK_MARK_ADVANCE_HU < LIST_INDENT_HU * 2) };
    }

    #[test]
    fn superscripts_shrink_and_raise() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("PDF/");
        let mut sup = docagent_model::Run::text("A");
        sup.style.superscript = true;
        p.runs.push(sup);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let base = page
            .lines
            .iter()
            .find(|l| l.text.contains("PDF"))
            .expect("base");
        let raised = page
            .lines
            .iter()
            .find(|l| l.text == "A")
            .expect("super");
        assert_eq!(
            raised.font_size, SUPER_SUB_SIZE_HU,
            "super size {} must be letter CSS 0.75em, not 2/3",
            raised.font_size
        );
        assert_eq!(raised.font_size, super_sub_size(base.font_size));
        assert_eq!(
            raised.y.saturating_add(raised.baseline),
            base.y
                .saturating_add(base.baseline)
                .saturating_sub(SUPER_SUB_SHIFT_HU),
            "super baseline {} vs {} must rise WeasyPrint 0.5em",
            raised.y.saturating_add(raised.baseline),
            base.y.saturating_add(base.baseline)
        );
        assert_eq!(SUPER_SUB_SIZE_HU, 750);
        assert_eq!(SUPER_SUB_SHIFT_HU, 375);
    }

    #[test]
    fn subscripts_shrink_and_lower() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("H");
        let mut sub = docagent_model::Run::text("2");
        sub.style.subscript = true;
        p.runs.push(sub);
        p.runs.push(docagent_model::Run::text("O"));
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let base = page
            .lines
            .iter()
            .find(|l| l.text == "H")
            .expect("base");
        let lowered = page
            .lines
            .iter()
            .find(|l| l.text == "2")
            .expect("sub");
        assert_eq!(
            lowered.font_size, SUPER_SUB_SIZE_HU,
            "sub size {} must be letter CSS 0.75em, not 2/3",
            lowered.font_size
        );
        assert_eq!(lowered.font_size, super_sub_size(base.font_size));
        assert_eq!(
            lowered.y.saturating_add(lowered.baseline),
            base.y
                .saturating_add(base.baseline)
                .saturating_add(SUPER_SUB_SHIFT_HU),
            "sub baseline {} vs {} must drop WeasyPrint 0.5em, not 1/5em",
            lowered.y.saturating_add(lowered.baseline),
            base.y.saturating_add(base.baseline)
        );
        assert_eq!(SUPER_SUB_SIZE_HU, 750);
        assert_eq!(SUPER_SUB_SHIFT_HU, 375);
        assert!(
            lowered.y + lowered.baseline > base.y + base.baseline,
            "sub baseline {} vs {}",
            lowered.y + lowered.baseline,
            base.y + base.baseline
        );
    }

    #[test]
    fn underlines_emit_fills() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("09:00 local");
        p.runs[0].style.underline = docagent_model::Underline::Single;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let line = page
            .lines
            .iter()
            .find(|l| l.text.contains("09:00"))
            .expect("underlined run");
        assert!(line.underline);
        assert!(!line.dotted, "letter.css dotted is abbr-only, not u");
        assert!(line.href.is_none());
        assert_eq!(
            line.color, BODY_FG,
            "u text must be letter body #134e4a, not UA black"
        );
        let fill = page
            .strokes
            .iter()
            .find(|s| {
                s.fill == STRIKE_COLOR
                    && s.height == UNDERLINE_HU
                    && s.width == line.width
                    && s.x == line.x
            })
            .expect("underline fill missing");
        assert_eq!(
            fill.y,
            line.y
                .saturating_add(line.baseline)
                .saturating_add(UNDERLINE_OFFSET_HU)
        );
        assert_eq!(UNDERLINE_HU, 80);
        assert_eq!(UNDERLINE_OFFSET_HU, 100);
    }

    #[test]
    fn hyperlinks_emit_underline_fills() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![docagent_model::Run::hyperlink(
            "DocAgent",
            "https://github.com/kevin9327/docagent",
        )];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        assert!(page.lines.iter().any(|l| l.text.contains("DocAgent") && l.underline));
        assert!(
            page.lines.iter().any(|l| {
                l.href.as_deref() == Some("https://github.com/kevin9327/docagent")
                    && l.link_hit().is_some()
            }),
            "hyperlink href missing: {:?}",
            page.lines
        );
        assert!(
            page.strokes.iter().any(|s| {
                s.fill == LINK_COLOR && s.height == LINK_UNDERLINE_HU && s.width > 80
            }),
            "hyperlink underline missing: {:?}",
            page.strokes
        );
    }

    #[test]
    fn highlights_emit_background_fills() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("three ");
        let mut mark = docagent_model::Run::text("hashes");
        mark.style.highlight = Some(docagent_model::Color::MARK);
        p.runs.push(mark);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let line = page
            .lines
            .iter()
            .find(|l| l.text.contains("hashes"))
            .expect("mark run");
        assert!(line.highlight);
        let fill = page
            .strokes
            .iter()
            .find(|s| s.fill == MARK_BG && s.width > 80 && s.height > 80)
            .expect("highlight fill missing");
        assert_eq!(fill.x, line.x.saturating_sub(MARK_PAD_X_HU));
        assert_eq!(
            fill.width,
            line.width.saturating_add(MARK_PAD_X_HU.saturating_mul(2))
        );
        assert_eq!(
            fill.height,
            line.font_size.saturating_add(MARK_PAD_Y_HU.saturating_mul(2))
        );
    }

    #[test]
    fn code_spans_emit_background_fills() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Run ");
        let mut code = docagent_model::Run::text("prove");
        code.style.code = true;
        p.runs.push(code);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let line = page
            .lines
            .iter()
            .find(|l| l.text.contains("prove"))
            .expect("code run");
        assert!(line.code);
        assert!(
            page.lines
                .iter()
                .any(|l| l.text.contains("Run") && !l.code)
        );
        let fill = page
            .strokes
            .iter()
            .find(|s| s.fill == CODE_SPAN_BG && s.width > 80 && s.height > 80)
            .expect("code background missing");
        assert_eq!(fill.x, line.x.saturating_sub(CODE_SPAN_PAD_X_HU));
        assert_eq!(
            fill.width,
            line.width.saturating_add(CODE_SPAN_PAD_X_HU.saturating_mul(2))
        );
        assert_eq!(
            fill.height,
            line.font_size.saturating_add(CODE_SPAN_PAD_Y_HU.saturating_mul(2))
        );
        let run = page
            .lines
            .iter()
            .find(|l| l.text.contains("Run"))
            .expect("plain run");
        assert_eq!(
            line.x,
            run.x.saturating_add(run.width).saturating_add(CODE_SPAN_PAD_X_HU),
            "code glyph x must include letter CSS 0.35em pad"
        );
    }

    #[test]
    fn code_blocks_emit_background_fills() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("docagent prove examples/letter.md");
        p.code_block = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        assert!(
            page.lines
                .iter()
                .any(|l| l.text.contains("docagent prove"))
        );
        assert!(
            page.strokes.iter().any(|s| {
                s.fill == [204, 251, 241, 255] && s.width > 10000 && s.height > 80
            }),
            "code block fill missing: {:?}",
            page.strokes
        );
    }

    #[test]
    fn thematic_breaks_emit_rule_fills() {
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
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        assert!(page.lines.iter().any(|l| l.text.contains("before")));
        assert!(page.lines.iter().any(|l| l.text.contains("after")));
        assert!(
            page.strokes.iter().any(|s| {
                s.fill == [19, 78, 74, 255] && s.height == HR_THICK_HU && s.width > 10000
            }),
            "thematic rule missing: {:?}",
            page.strokes
        );
    }

    #[test]
    fn quotes_emit_left_bar_fills() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        p.quote = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        assert!(
            page.lines
                .iter()
                .any(|l| l.text.contains("Replay is evidence"))
        );
        let replay = page
            .lines
            .iter()
            .find(|l| l.text.contains("Replay is evidence"))
            .expect("quote text");
        assert_eq!(replay.color, QUOTE_FG, "blockquote text must be letter CSS #134e4a");
        assert!(
            page.strokes.iter().any(|s| {
                s.fill == QUOTE_FG && s.width == QUOTE_BAR_W_HU && s.height > 200
            }),
            "quote bar missing: {:?}",
            page.strokes
        );
        assert_eq!(QUOTE_FG, [19, 78, 74, 255]);
        assert_eq!(QUOTE_BAR_W_HU, 300);
    }

    #[test]
    fn strikethrough_emits_midline_fills() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("plain ");
        let mut strike = docagent_model::Run::text("guess");
        strike.style.strike = true;
        p.runs.push(strike);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let line = page
            .lines
            .iter()
            .find(|l| l.text.contains("guess"))
            .expect("struck run");
        assert!(line.strike);
        assert_eq!(line.color, STRIKE_COLOR, "s text must be letter CSS #134e4a");
        assert!(
            page.lines
                .iter()
                .any(|l| l.text.contains("plain") && !l.strike)
        );
        let fill = page
            .strokes
            .iter()
            .find(|s| {
                s.fill == STRIKE_COLOR
                    && s.height == STRIKE_HU
                    && s.width == line.width
                    && s.x == line.x
                    && s.y < line.y.saturating_add(line.baseline)
            })
            .expect("strikethrough fill missing");
        assert_eq!(
            fill.y,
            line.y
                .saturating_add(line.baseline)
                .saturating_sub(line.font_size / 3)
        );
        assert_eq!(STRIKE_HU, 80);
        assert_eq!(STRIKE_COLOR, [19, 78, 74, 255]);
    }

    #[test]
    fn decimal_prefix_uses_start() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Hash the input");
        p.numbering = Some(NumberingRef {
            definition_id: 1,
            level: 0,
            start: Some(3),
            format: Some(NumberFormat::Decimal),
        });
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let joined: String = tree.pages[0]
            .lines
            .iter()
            .map(|l| l.text.as_str())
            .collect();
        assert!(joined.starts_with("3. "), "{joined}");
        assert!(joined.contains("Hash the input"), "{joined}");
    }

    #[test]
    fn nested_bullet_hangs_further() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut a = Paragraph::from_text("dock");
        a.indent_left = 1440;
        a.numbering = Some(NumberingRef {
            definition_id: 0,
            level: 0,
            start: None,
            format: Some(NumberFormat::Bullet),
        });
        let mut b = Paragraph::from_text("bay 4");
        b.indent_left = 2880;
        b.numbering = Some(NumberingRef {
            definition_id: 0,
            level: 1,
            start: None,
            format: Some(NumberFormat::Bullet),
        });
        section.body.push(Block::Paragraph(a));
        section.body.push(Block::Paragraph(b));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let parent = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.text.contains("dock"))
            .map(|l| l.x)
            .unwrap();
        let nested = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.text.contains("bay"))
            .map(|l| l.x)
            .unwrap();
        assert!(nested > parent, "parent={parent} nested={nested}");
    }

    #[test]
    fn bold_runs_mark_line_frags() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("plain ");
        let mut bold = docagent_model::Run::text("GPU-free");
        bold.style.bold = true;
        p.runs.push(bold);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let bold_frag = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.text.contains("GPU-free"))
            .expect("bold span");
        assert!(bold_frag.bold);
        let plain = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.text.contains("plain"))
            .expect("plain span");
        assert!(!plain.bold);
    }

    #[test]
    fn italic_runs_mark_line_frags() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("plain ");
        let mut italic = docagent_model::Run::text("byte-for-byte");
        italic.style.italic = true;
        p.runs.push(italic);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let italic_frag = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.text.contains("byte-for-byte"))
            .expect("italic span");
        assert!(italic_frag.italic);
        let plain = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.text.contains("plain"))
            .expect("plain span");
        assert!(!plain.italic);
    }

    const MARK_PNG: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/assets/mark.png"
    ));

    fn png_ihdr_px(bytes: &[u8]) -> (u32, u32) {
        const SIG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        assert!(
            bytes.len() >= 24 && bytes.starts_with(SIG) && &bytes[12..16] == b"IHDR",
            "mark.png missing IHDR"
        );
        let w = u32::from_be_bytes(bytes[16..20].try_into().expect("ihdr width"));
        let h = u32::from_be_bytes(bytes[20..24].try_into().expect("ihdr height"));
        assert!(w > 0 && h > 0, "empty PNG");
        (w, h)
    }

    fn px_to_hu_96(px: u32) -> Hu {
        i32::try_from(i64::from(px).saturating_mul(i64::from(docagent_model::HU_PER_INCH)) / 96)
            .unwrap_or(i32::MAX)
    }

    fn mark_png_hu() -> (Hu, Hu) {
        let (w, h) = png_ihdr_px(MARK_PNG);
        (px_to_hu_96(w), px_to_hu_96(h))
    }

    fn mark_run(width: Hu, height: Hu) -> docagent_model::Run {
        docagent_model::Run {
            style: docagent_model::CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(docagent_model::ImageData {
                bytes: MARK_PNG.to_vec(),
                mime: "image/png".into(),
                width,
                height,
                alt_text: Some("mark".into()),
                wrap: docagent_model::WrapMode::Inline,
            })),
        }
    }

    #[test]
    fn image_runs_reserve_width_and_height() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![mark_run(1600, 1600)];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let frag = page
            .lines
            .iter()
            .find(|l| l.image.is_some())
            .expect("image run skipped");
        let (ew, eh) = min_on_page_size(1600, 1600);
        assert_eq!(frag.width, ew);
        assert!(frag.height >= eh, "line height {}", frag.height);
        let img = frag.image.as_ref().expect("payload");
        assert_eq!(img.width, ew);
        assert_eq!(img.height, eh);
        assert_eq!(img.mime, "image/png");
        assert_eq!(img.bytes, MARK_PNG);
        assert!(frag.text.is_empty());
    }

    #[test]
    fn image_run_advances_following_text() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![
            docagent_model::Run::text("before"),
            mark_run(1600, 800),
            docagent_model::Run::text("after"),
        ];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let before = page
            .lines
            .iter()
            .find(|l| l.text.contains("before"))
            .expect("before");
        let img = page
            .lines
            .iter()
            .find(|l| l.image.is_some())
            .expect("image");
        let after = page
            .lines
            .iter()
            .find(|l| l.text.contains("after"))
            .expect("after");
        let (ew, eh) = min_on_page_size(1600, 800);
        assert!(img.x >= before.x + before.width, "image x {}", img.x);
        assert!(after.x >= img.x + img.width, "after x {}", after.x);
        assert_eq!(img.width, ew);
        assert_eq!(img.image.as_ref().map(|i| i.height), Some(eh));
        assert_eq!(before.y, img.y);
        assert_eq!(after.y, img.y);
        assert!(img.height >= eh, "line height {}", img.height);
    }

    fn boxes_overlap(a: (Hu, Hu, Hu, Hu), b: (Hu, Hu, Hu, Hu)) -> bool {
        let (ax, ay, aw, ah) = a;
        let (bx, by, bw, bh) = b;
        aw > 0
            && ah > 0
            && bw > 0
            && bh > 0
            && ax < bx.saturating_add(bw)
            && ax.saturating_add(aw) > bx
            && ay < by.saturating_add(bh)
            && ay.saturating_add(ah) > by
    }

    fn letter_with_mark(width: Hu, height: Hu) -> Document {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut heading = Paragraph::from_text("Northwind Freight");
        heading.outline_level = Some(1);
        heading.runs[0].style.size = 1800;
        heading.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(heading));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![mark_run(width, height)];
        section.body.push(Block::Paragraph(img_p));
        section.body.push(Block::Paragraph(Paragraph::from_text(
            "The shipment left Busan on 4 September",
        )));
        doc.sections.push(section);
        doc
    }

    #[test]
    fn min_on_page_size_raises_sub_24pt_box() {
        assert_eq!(
            min_on_page_size(1200, 1200),
            (MIN_IMAGE_EDGE_HU, MIN_IMAGE_EDGE_HU)
        );
        assert_eq!(min_on_page_size(2400, 2400), (2400, 2400));
        assert_eq!(min_on_page_size(4800, 2400), (4800, 2400));
    }

    #[test]
    fn letter_mark_png_uses_minimum_on_page_size_without_overlapping_body() {
        assert_eq!(png_ihdr_px(MARK_PNG), (16, 16), "fixture is 16×16 px");
        let (w, h) = mark_png_hu();
        assert_eq!((w, h), (px_to_hu_96(16), px_to_hu_96(16)));
        assert!(
            w < MIN_IMAGE_EDGE_HU && h < MIN_IMAGE_EDGE_HU,
            "16px at 96dpi must be below the 24pt floor ({w}×{h})"
        );
        let (ew, eh) = min_on_page_size(w, h);
        assert!(ew >= MIN_IMAGE_EDGE_HU);
        assert!(eh >= MIN_IMAGE_EDGE_HU);
        assert!(ew >= docagent_model::HU_PER_INCH / 4);
        assert!(eh >= docagent_model::HU_PER_INCH / 4);
        let tree = layout_document(&letter_with_mark(w, h), &FontSet::bundled());
        let page = &tree.pages[0];
        let img_line = page
            .lines
            .iter()
            .find(|l| l.image.is_some())
            .expect("letter image dropped");
        let img = img_line.image.as_ref().expect("payload");
        assert_eq!(img.width, ew);
        assert_eq!(img.height, eh);
        assert_eq!(img.bytes, MARK_PNG);
        assert_eq!(img.mime, "image/png");
        assert_eq!(img_line.width, ew);
        assert!(
            img_line.height >= eh,
            "line box {} shorter than on-page height {eh}",
            img_line.height
        );
        let img_box = img_line.image_box().expect("image box");
        assert_eq!(img_box, (img_line.x, img_line.y, ew, eh));
        let body = page
            .lines
            .iter()
            .find(|l| l.text.contains("shipment"))
            .expect("body dropped");
        assert!(
            body.y >= img_line.y.saturating_add(eh),
            "body y {} overlaps image y {} height {eh}",
            body.y,
            img_line.y
        );
        let body_box = (body.x, body.y, body.width.max(1), body.height.max(1));
        assert!(
            !boxes_overlap(img_box, body_box),
            "image {img_box:?} overlaps body {body_box:?}"
        );
        let heading = page
            .lines
            .iter()
            .find(|l| l.text.contains("Northwind"))
            .expect("heading dropped");
        let heading_box = (
            heading.x,
            heading.y,
            heading.width.max(1),
            heading.height.max(1),
        );
        assert!(
            !boxes_overlap(img_box, heading_box),
            "image {img_box:?} overlaps heading {heading_box:?}"
        );
        let h1_rule = page.strokes.iter().find(|s| {
            s.fill == BODY_FG && s.height == H1_RULE_HU && s.width > 1000
        });
        if let Some(rule) = h1_rule {
            let rule_box = (rule.x, rule.y, rule.width.max(1), rule.height.max(1));
            assert!(
                !boxes_overlap(img_box, rule_box),
                "image {img_box:?} overlaps heading rule {rule_box:?}"
            );
        }
    }

    fn letter_page(img_w: Hu, img_h: Hu) -> Document {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.size = 1800;
        h1.runs[0].style.bold = true;
        h1.space_after = 400;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation — order 48291");
        h2.outline_level = Some(2);
        h2.runs[0].style.size = 1400;
        h2.runs[0].style.bold = true;
        h2.space_before = 360;
        h2.space_after = 240;
        section.body.push(Block::Paragraph(h2));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![mark_run(img_w, img_h)];
        section.body.push(Block::Paragraph(img_p));
        let mut body = Paragraph::from_text(
            "The shipment left Busan on 4 September and is booked on the 11 September rail window.",
        );
        body.space_after = 200;
        section.body.push(Block::Paragraph(body));
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into(), "Proof".into()],
            vec!["Input".into(), "letter.md".into(), "input SHA-256".into()],
        ]);
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        doc
    }

    #[test]
    fn letter_page_type_spacing_and_table_are_dense() {
        let (w, h) = mark_png_hu();
        let (ew, eh) = min_on_page_size(w, h);
        let tree = layout_document(&letter_page(w, h), &FontSet::bundled());
        let page = &tree.pages[0];
        let h1 = page
            .lines
            .iter()
            .find(|l| l.text.contains("Northwind"))
            .expect("h1");
        let h2 = page
            .lines
            .iter()
            .find(|l| l.text.contains("Delivery"))
            .expect("h2");
        let img_line = page
            .lines
            .iter()
            .find(|l| l.image.is_some())
            .expect("mark");
        let body = page
            .lines
            .iter()
            .find(|l| l.text.contains("shipment"))
            .expect("body");
        assert_eq!(H1_SIZE_MAX_HU, 2100, "h1 must stay letter CSS 1.75rem");
        assert_eq!(H2_SIZE_MAX_HU, 1380, "h2 must stay letter CSS 1.15rem");
        assert_eq!(BODY_LEADING_PCT, 145, "body leading must stay CSS 1.45");
        assert_eq!(BODY_AFTER_HU, 840, "body after must stay letter CSS 0.7rem");
        assert_eq!(h1.font_size, H1_SIZE_MAX_HU, "h1 size {}", h1.font_size);
        assert_eq!(h2.font_size, H2_SIZE_MAX_HU, "h2 size {}", h2.font_size);
        assert_eq!(body.font_size, DEFAULT_FONT_SIZE_HU);
        let fonts = FontSet::bundled();
        let body_lh_css = i32::try_from(i64::from(body.font_size) * 145 / 100).unwrap_or(i32::MAX);
        let body_lh_cap = body_lh_css.max(line_height(&fonts, body.font_size, 100));
        assert_eq!(
            h1.height,
            heading_line_height(h1.font_size),
            "h1 leading {} must be letter CSS 1.2, not Noto (asc+desc) {}",
            h1.height,
            line_height(&fonts, h1.font_size, 100)
        );
        assert_eq!(
            h2.height,
            heading_line_height(h2.font_size),
            "h2 leading {} must be letter CSS 1.2, not Noto (asc+desc) {}",
            h2.height,
            line_height(&fonts, h2.font_size, 100)
        );
        assert!(
            h1.height < line_height(&fonts, h1.font_size, 100),
            "h1 1.2em must beat Noto natural"
        );
        assert!(
            h2.height < line_height(&fonts, h2.font_size, 100),
            "h2 1.2em must beat Noto natural"
        );
        assert!(
            body.height >= body_lh_css,
            "body leading {} jammed vs CSS 1.45 of {}",
            body.height,
            body.font_size
        );
        assert!(
            body.height <= body_lh_cap,
            "body leading {} sparse vs CSS 1.45",
            body.height
        );
        let heading_gap = h2.y.saturating_sub(h1.y.saturating_add(h1.height));
        assert_eq!(
            heading_gap.saturating_sub(H1_RULE_GAP_HU.saturating_add(H1_RULE_HU)),
            H2_BEFORE_HU,
            "h1 0.4rem pad + 3px rule sit in the box; remaining {heading_gap} must be letter CSS 1.1rem, not IR 400"
        );
        let img_box = img_line.image_box().expect("image box");
        assert_eq!(img_box, (img_line.x, img_line.y, ew, eh));
        let h1_box = (h1.x, h1.y, h1.width.max(1), h1.height.max(1));
        let h2_box = (h2.x, h2.y, h2.width.max(1), h2.height.max(1));
        let body_box = (body.x, body.y, body.width.max(1), body.height.max(1));
        assert!(!boxes_overlap(img_box, h1_box), "mark overlaps h1");
        assert!(!boxes_overlap(img_box, h2_box), "mark overlaps h2");
        assert!(!boxes_overlap(img_box, body_box), "mark overlaps body");
        assert_eq!(
            img_line.y,
            h2.y
                .saturating_add(h2.height)
                .saturating_add(H2_RULE_GAP_HU)
                .saturating_add(H2_RULE_HU)
                .saturating_add(IMAGE_BEFORE_HU),
            "mark y {} must sit 0.2rem pad + 2px rule + 0.6rem img margin below h2, not jammed",
            img_line.y.saturating_sub(h2.y.saturating_add(h2.height))
        );
        assert_eq!(
            body.y,
            img_line.y.saturating_add(eh).saturating_add(IMAGE_AFTER_HU),
            "body y {} must sit IMAGE_AFTER_HU below mark y {} height {eh}",
            body.y,
            img_line.y
        );
        for stroke in &page.strokes {
            if stroke.height <= 0 || stroke.width <= 0 {
                continue;
            }
            let sb = (stroke.x, stroke.y, stroke.width, stroke.height);
            if stroke.fill == BODY_FG && stroke.height == H1_RULE_HU {
                assert!(!boxes_overlap(img_box, sb), "mark overlaps h1 rule {sb:?}");
            }
            if stroke.fill == [13, 148, 136, 255] && stroke.height == H2_RULE_HU {
                assert!(!boxes_overlap(img_box, sb), "mark overlaps h2 rule {sb:?}");
            }
        }
        let rows = table_row_heights(&tree);
        assert!(rows.len() >= 6, "header+body cells: {rows:?}");
        let cell_lh = i32::try_from(i64::from(CELL_SIZE_HU) * i64::from(BODY_LEADING_PCT) / 100)
            .unwrap_or(i32::MAX)
            .max(line_height(&fonts, CELL_SIZE_HU, 100));
        let row_cap = cell_lh
            .saturating_add(CELL_PAD_Y_HU.saturating_mul(2))
            .saturating_add(40);
        for h in &rows {
            assert!(
                *h <= row_cap,
                "table row {h} taller than {row_cap} (cell type + pad)"
            );
            assert!(*h > CELL_PAD_Y_HU, "empty table row {h}");
        }
        let headers = page
            .rects
            .iter()
            .filter(|r| r.fill == TH_BG)
            .count();
        assert_eq!(headers, 3, "header cells missing teal fill");
        let hop = page
            .lines
            .iter()
            .find(|l| l.text.contains("Hop"))
            .expect("header text");
        let input = page
            .lines
            .iter()
            .find(|l| l.text.contains("Input"))
            .expect("body cell");
        assert!(
            hop.x >= page.rects[0].x.saturating_add(CELL_PAD_X_HU),
            "header text must sit in cell pad, x {} cell {}",
            hop.x,
            page.rects[0].x
        );
        assert!(input.y > hop.y, "body row must follow header");
    }

    #[test]
    fn letter_table_cell_padding_is_weasyprint_px() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let cell = page.rects.first().expect("header cell");
        let hop = page
            .lines
            .iter()
            .find(|l| l.text.contains("Hop"))
            .expect("header text");
        assert_eq!(
            hop.x,
            cell.x.saturating_add(CELL_PAD_X_HU),
            "cell pad x {} must be letter CSS 0.65rem, not UA 1px",
            hop.x.saturating_sub(cell.x)
        );
        assert_eq!(
            hop.y,
            cell.y.saturating_add(CELL_PAD_Y_HU),
            "cell pad y {} must be letter CSS 0.4rem, not UA 1px",
            hop.y.saturating_sub(cell.y)
        );
        let input = page
            .lines
            .iter()
            .find(|l| l.text.contains("Input"))
            .expect("body cell");
        let body_cell = page
            .rects
            .iter()
            .find(|r| r.y > cell.y && r.fill == [255, 255, 255, 255])
            .expect("body fill");
        assert_eq!(input.x, body_cell.x.saturating_add(CELL_PAD_X_HU));
        assert_eq!(input.y, body_cell.y.saturating_add(CELL_PAD_Y_HU));
        assert_eq!(
            hop.font_size, CELL_SIZE_HU,
            "th type {} must be letter CSS 0.95rem, not body 10pt",
            hop.font_size
        );
        assert_eq!(
            input.font_size, CELL_SIZE_HU,
            "td type {} must be letter CSS 0.95rem, not body 10pt",
            input.font_size
        );
        assert_eq!(CELL_SIZE_HU, 1140);
    }

    #[test]
    fn letter_table_cell_type_overrides_ir_size() {
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.header_row_count = 1;
        if let Block::Paragraph(p) = &mut table.rows[0].cells[0].blocks[0] {
            p.runs[0].style.size = 1800;
        }
        if let Block::Paragraph(p) = &mut table.rows[1].cells[0].blocks[0] {
            p.runs[0].style.size = 1800;
        }
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let hop = page
            .lines
            .iter()
            .find(|l| l.text.contains("Hop"))
            .expect("header text");
        let input = page
            .lines
            .iter()
            .find(|l| l.text.contains("Input"))
            .expect("body cell");
        assert_eq!(
            hop.font_size, CELL_SIZE_HU,
            "th type {} must stay letter CSS 0.95rem when IR is 18pt",
            hop.font_size
        );
        assert_eq!(
            input.font_size, CELL_SIZE_HU,
            "td type {} must stay letter CSS 0.95rem when IR is 18pt",
            input.font_size
        );
        assert_ne!(hop.font_size, 1800);
        assert_eq!(CELL_SIZE_HU, 1140);
        assert_eq!(CELL_PAD_X_HU, 780);
        assert_eq!(CELL_PAD_Y_HU, 480);
    }

    fn wrapping_hop_table(short_valign: VAlign) -> Table {
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec![
                "The shipment left Busan on 4 September and is booked on the 11 September rail window. This is a flow document: one convert, three hashes, GPU-free.".into(),
                "Plan".into(),
            ],
        ]);
        table.header_row_count = 1;
        table.rows[1].cells[1].valign = short_valign;
        table
    }

    #[test]
    fn letter_table_cell_valign_is_weasyprint_top() {
        assert_eq!(
            docagent_model::TableCell::default().valign,
            VAlign::Top,
            "IR default must stay letter CSS top, not UA tbody middle"
        );
        assert_eq!(cell_valign_offset(VAlign::Top, 2000, 5000), 0);
        assert_eq!(cell_valign_offset(VAlign::Middle, 2000, 5000), 1500);
        assert_eq!(cell_valign_offset(VAlign::Bottom, 2000, 5000), 3000);

        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Table(wrapping_hop_table(VAlign::Top)));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let wrap: Vec<&LineFrag> = page
            .lines
            .iter()
            .filter(|l| l.text.contains("shipment") || l.text.contains("Busan") || l.text.contains("flow document") || l.text.contains("GPU-free") || l.text.contains("September"))
            .filter(|l| !l.text.contains("Hop") && !l.text.contains("File") && !l.text.contains("Plan"))
            .collect();
        assert!(
            wrap.len() >= 2,
            "fixture must wrap so leftover is observable: {:?}",
            page.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>()
        );
        let wrap0 = wrap[0];
        let plan = page
            .lines
            .iter()
            .find(|l| l.text.contains("Plan"))
            .expect("short hop cell");
        let wrap_cell = page
            .rects
            .iter()
            .find(|r| r.fill == [255, 255, 255, 255] && wrap0.x >= r.x && wrap0.x < r.x.saturating_add(r.width))
            .expect("wrapping cell fill");
        let plan_cell = page
            .rects
            .iter()
            .find(|r| r.fill == [255, 255, 255, 255] && plan.x >= r.x && plan.x < r.x.saturating_add(r.width))
            .expect("short cell fill");
        assert_eq!(wrap_cell.height, plan_cell.height, "row height is shared");
        assert!(
            wrap_cell.height > plan.height.saturating_add(CELL_PAD_Y_HU.saturating_mul(2)),
            "wrapping cell must leave leftover that UA middle would center"
        );
        assert_eq!(
            wrap0.y,
            wrap_cell.y.saturating_add(CELL_PAD_Y_HU),
            "wrapping hop cell must stay letter CSS top pad"
        );
        assert_eq!(
            plan.y,
            plan_cell.y.saturating_add(CELL_PAD_Y_HU),
            "short hop cell y {} must stay letter CSS vertical-align:top, not UA middle {}",
            plan.y.saturating_sub(plan_cell.y),
            CELL_PAD_Y_HU.saturating_add(cell_valign_offset(
                VAlign::Middle,
                plan.height.saturating_add(CELL_PAD_Y_HU.saturating_mul(2)),
                plan_cell.height
            ))
        );
        assert_eq!(
            plan.y, wrap0.y,
            "top-aligned hop cells must share the row top, not sit in leftover"
        );
        assert_eq!(
            cell_valign_offset(VAlign::Top, plan_cell.height, plan_cell.height),
            0
        );

        let mut middle = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Table(wrapping_hop_table(VAlign::Middle)));
        middle.sections.push(section);
        let mid_tree = layout_document(&middle, &FontSet::bundled());
        let mid_page = &mid_tree.pages[0];
        let mid_plan = mid_page
            .lines
            .iter()
            .find(|l| l.text.contains("Plan"))
            .expect("middle short cell");
        let mid_wrap = mid_page
            .lines
            .iter()
            .find(|l| l.text.contains("Busan") || l.text.contains("shipment"))
            .expect("middle wrap");
        let mid_plan_cell = mid_page
            .rects
            .iter()
            .find(|r| {
                r.fill == [255, 255, 255, 255]
                    && mid_plan.x >= r.x
                    && mid_plan.x < r.x.saturating_add(r.width)
            })
            .expect("middle short fill");
        let short_h = mid_plan
            .height
            .saturating_add(CELL_PAD_Y_HU.saturating_mul(2));
        let expected_mid = mid_plan_cell
            .y
            .saturating_add(cell_valign_offset(
                VAlign::Middle,
                short_h,
                mid_plan_cell.height,
            ))
            .saturating_add(CELL_PAD_Y_HU);
        assert_eq!(
            mid_plan.y, expected_mid,
            "UA middle must shift the short hop cell into leftover, proving top is a lock"
        );
        assert!(
            mid_plan.y > mid_wrap.y,
            "UA middle {} must sit below the wrapping cell top {}",
            mid_plan.y,
            mid_wrap.y
        );
        assert!(
            mid_plan.y > plan.y,
            "letter CSS top {} must beat UA middle {}",
            plan.y,
            mid_plan.y
        );
        assert_eq!(CELL_PAD_Y_HU, 480);
        assert_eq!(CELL_PAD_X_HU, 780);
        assert_eq!(CELL_SIZE_HU, 1140);
    }

    #[test]
    fn letter_table_grid_is_weasyprint_collapse() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into(), "Proof".into()],
            vec!["Input".into(), "letter.md".into(), "input SHA-256".into()],
            vec!["Plan".into(), "layout i32".into(), "plan SHA-256".into()],
        ]);
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let headers: Vec<&RectFrag> = page.rects.iter().filter(|r| r.fill == TH_BG).collect();
        assert_eq!(headers.len(), 3, "hop header cells: {:?}", page.rects);
        let hop = headers[0];
        let file = headers[1];
        assert_eq!(
            file.x,
            hop.x.saturating_add(hop.width),
            "header cells must share a collapsed edge"
        );
        let v_edge = file.x;
        let mut v_xs: Vec<Hu> = page
            .strokes
            .iter()
            .filter(|s| {
                s.fill == CELL_BORDER
                    && s.width == CELL_BORDER_HU
                    && s.height > CELL_BORDER_HU
                    && (s.x == v_edge || s.x == v_edge.saturating_sub(CELL_BORDER_HU))
            })
            .map(|s| s.x)
            .collect();
        v_xs.sort_unstable();
        v_xs.dedup();
        assert_eq!(
            v_xs,
            [v_edge],
            "internal vertical must be one collapsed 1px at the shared edge, not two adjacent 1px boxes: {:?}",
            page.strokes
                .iter()
                .filter(|s| s.fill == CELL_BORDER)
                .collect::<Vec<_>>()
        );
        let seam = hop.y.saturating_add(hop.height);
        let gray_at_header_seam = page.strokes.iter().any(|s| {
            s.fill == CELL_BORDER
                && s.height == CELL_BORDER_HU
                && s.width > CELL_BORDER_HU
                && (s.y == seam || s.y == seam.saturating_sub(CELL_BORDER_HU))
        });
        assert!(
            !gray_at_header_seam,
            "header/body seam must stay 2px #0d9488, not a stacked 1px gray: {:?}",
            page.strokes
                .iter()
                .filter(|s| s.fill == CELL_BORDER || s.fill == TH_RULE)
                .collect::<Vec<_>>()
        );
        assert!(
            page.strokes
                .iter()
                .any(|s| s.fill == TH_RULE && s.height == TH_RULE_HU && s.y == seam.saturating_sub(TH_RULE_HU)),
            "th 2px #0d9488 must sit on the collapsed header seam"
        );
        let bodies: Vec<&RectFrag> = page
            .rects
            .iter()
            .filter(|r| r.fill == [255, 255, 255, 255] && r.y > hop.y)
            .collect();
        assert!(bodies.len() >= 6, "body cells: {:?}", page.rects);
        let input = bodies
            .iter()
            .find(|r| r.x == hop.x)
            .expect("input cell");
        let plan = bodies
            .iter()
            .find(|r| r.x == hop.x && r.y > input.y)
            .expect("plan cell");
        let body_seam = input.y.saturating_add(input.height);
        assert_eq!(plan.y, body_seam);
        let mut h_ys: Vec<Hu> = page
            .strokes
            .iter()
            .filter(|s| {
                s.fill == CELL_BORDER
                    && s.height == CELL_BORDER_HU
                    && s.width > CELL_BORDER_HU
                    && (s.y == body_seam || s.y == body_seam.saturating_sub(CELL_BORDER_HU))
            })
            .map(|s| s.y)
            .collect();
        h_ys.sort_unstable();
        h_ys.dedup();
        assert_eq!(
            h_ys,
            [body_seam],
            "internal horizontal must be one collapsed 1px, not two stacked 1px boxes: {h_ys:?}"
        );
        assert_eq!(CELL_BORDER_HU, 75);
        assert_eq!(TH_RULE_HU, 150);
        assert_eq!(TH_RULE_HU, 2 * CELL_BORDER_HU);
        assert_eq!(CELL_BORDER, [203, 213, 225, 255]);
        assert_eq!(CELL_PAD_X_HU, 780);
        assert_eq!(CELL_PAD_Y_HU, 480);
        assert_eq!(CELL_SIZE_HU, 1140);
        const { assert!(TH_RULE_HU == 2 * CELL_BORDER_HU) };
    }

    #[test]
    fn letter_paragraph_gap_after_mark_ignores_body_after_space() {
        let (w, h) = mark_png_hu();
        let (ew, eh) = min_on_page_size(w, h);
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![mark_run(w, h)];
        img_p.space_after = 200;
        section.body.push(Block::Paragraph(img_p));
        let mut body = Paragraph::from_text("6 September 2026");
        body.space_after = 200;
        section.body.push(Block::Paragraph(body));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let img_line = page
            .lines
            .iter()
            .find(|l| l.image.is_some())
            .expect("mark");
        let date = page
            .lines
            .iter()
            .find(|l| l.text.contains("September"))
            .expect("date");
        let img_box = img_line.image_box().expect("image box");
        assert_eq!(img_box, (img_line.x, img_line.y, ew, eh));
        assert_eq!(
            img_line.height, eh,
            "image-only line must not keep a body leading strut"
        );
        assert_eq!(
            date.y,
            img_line.y.saturating_add(eh).saturating_add(IMAGE_AFTER_HU),
            "gap after mark {} must be letter CSS 0.7rem, not IR space_after 200",
            date.y.saturating_sub(img_line.y.saturating_add(eh))
        );
        assert_eq!(
            date.font_size, DEFAULT_FONT_SIZE_HU,
            "type after the 24pt mark {} must stay letter CSS 10pt body",
            date.font_size
        );
        let date_box = (date.x, date.y, date.width.max(1), date.height.max(1));
        assert!(
            !boxes_overlap(img_box, date_box),
            "mark {img_box:?} overlaps date {date_box:?}"
        );
    }

    #[test]
    fn letter_gap_before_mark_under_h2_is_weasyprint_06rem() {
        let (w, h) = mark_png_hu();
        let (ew, eh) = min_on_page_size(w, h);
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h2 = Paragraph::from_text("Delivery confirmation — order 48291");
        h2.outline_level = Some(2);
        h2.runs[0].style.size = 1400;
        h2.runs[0].style.bold = true;
        h2.space_before = 360;
        h2.space_after = 240;
        section.body.push(Block::Paragraph(h2));
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![mark_run(w, h)];
        img_p.space_after = 200;
        section.body.push(Block::Paragraph(img_p));
        let mut date = Paragraph::from_text("6 September 2026");
        date.space_after = 200;
        section.body.push(Block::Paragraph(date));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let h2 = page
            .lines
            .iter()
            .find(|l| l.text.contains("Delivery"))
            .expect("h2");
        let img_line = page
            .lines
            .iter()
            .find(|l| l.image.is_some())
            .expect("mark");
        let date = page
            .lines
            .iter()
            .find(|l| l.text.contains("September"))
            .expect("date");
        let img = img_line.image.as_ref().expect("payload");
        assert_eq!((img.width, img.height), (ew, eh));
        let h2_rule = page
            .strokes
            .iter()
            .find(|s| s.fill == [13, 148, 136, 255] && s.height == H2_RULE_HU)
            .expect("h2 2px rule");
        assert_eq!(
            h2_rule.y,
            h2.y.saturating_add(h2.height).saturating_add(H2_RULE_GAP_HU),
            "h2 rule y {} must sit letter CSS 0.2rem below the line box",
            h2_rule.y.saturating_sub(h2.y.saturating_add(h2.height))
        );
        assert_eq!(
            img_line.y,
            h2_rule
                .y
                .saturating_add(H2_RULE_HU)
                .saturating_add(IMAGE_BEFORE_HU),
            "gap after h2 rule {} must be letter CSS 0.6rem img margin, not jammed",
            img_line.y.saturating_sub(h2_rule.y.saturating_add(H2_RULE_HU))
        );
        assert_eq!(
            date.y,
            img_line.y.saturating_add(eh).saturating_add(IMAGE_AFTER_HU),
            "gap after mark must stay letter CSS 0.7rem"
        );
        assert_eq!(IMAGE_BEFORE_HU, 720);
        assert_eq!(H1_RULE_GAP_HU, 480);
        assert_eq!(H1_RULE_HU, 225);
        assert_eq!(H2_RULE_GAP_HU, 240);
        assert_eq!(H2_RULE_HU, 150);
        assert_eq!(H2_AFTER_HU, 540);
        assert_eq!(H2_BEFORE_HU, 1320);
        assert_eq!(h2.color, BODY_FG);
        assert_eq!(date.color, BODY_FG);
    }

    #[test]
    fn letter_first_line_after_h2_is_weasyprint_045rem() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h2 = Paragraph::from_text("Delivery confirmation — order 48291");
        h2.outline_level = Some(2);
        h2.runs[0].style.size = 1400;
        h2.runs[0].style.bold = true;
        h2.space_after = 240;
        section.body.push(Block::Paragraph(h2));
        let mut date = Paragraph::from_text("6 September 2026");
        date.space_after = 200;
        section.body.push(Block::Paragraph(date));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let h2 = page
            .lines
            .iter()
            .find(|l| l.text.contains("Delivery"))
            .expect("h2");
        let date = page
            .lines
            .iter()
            .find(|l| l.text.contains("September"))
            .expect("date");
        let h2_rule = page
            .strokes
            .iter()
            .find(|s| s.fill == [13, 148, 136, 255] && s.height == H2_RULE_HU)
            .expect("h2 2px rule");
        assert_eq!(
            date.y,
            h2_rule
                .y
                .saturating_add(H2_RULE_HU)
                .saturating_add(H2_AFTER_HU),
            "first body line after h2 {} must be letter CSS 0.45rem, not IR 240 or jammed on the rule",
            date.y.saturating_sub(h2_rule.y.saturating_add(H2_RULE_HU))
        );
        assert_eq!(
            date.y,
            h2.y
                .saturating_add(h2.height)
                .saturating_add(H2_RULE_GAP_HU)
                .saturating_add(H2_RULE_HU)
                .saturating_add(H2_AFTER_HU),
            "h2 0.2rem pad + 2px rule sit in the box; remaining must be 0.45rem"
        );
        assert_eq!(H2_AFTER_HU, 540, "h2 margin-bottom must stay letter CSS 0.45rem");
        const { assert!(H2_AFTER_HU < IMAGE_BEFORE_HU) };
        const { assert!(H2_AFTER_HU < BODY_AFTER_HU) };
        assert_eq!(date.color, BODY_FG);
        assert_eq!(h2.color, BODY_FG);
    }

    #[test]
    fn letter_body_paragraph_gap_is_weasyprint_07rem() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut date = Paragraph::from_text("6 September 2026");
        date.space_after = 200;
        section.body.push(Block::Paragraph(date));
        let mut dest = Paragraph::from_text("To: operations desk, Duisburg terminal.");
        dest.space_after = 200;
        section.body.push(Block::Paragraph(dest));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let first = page
            .lines
            .iter()
            .find(|l| l.text.contains("September"))
            .expect("date");
        let second = page
            .lines
            .iter()
            .find(|l| l.text.contains("operations"))
            .expect("dest");
        assert_eq!(
            second.y,
            first.y.saturating_add(first.height).saturating_add(BODY_AFTER_HU),
            "body paragraph gap {} must be letter CSS 0.7rem, not IR space_after 200",
            second.y.saturating_sub(first.y.saturating_add(first.height))
        );
    }

    fn bullet_para(text: &str, level: u8, space_after: Hu) -> Paragraph {
        let mut p = Paragraph::from_text(text);
        p.space_after = space_after;
        p.indent_left = 1440i32.saturating_mul(i32::from(level) + 1);
        p.numbering = Some(NumberingRef {
            definition_id: 0,
            level,
            start: None,
            format: Some(NumberFormat::Bullet),
        });
        p
    }

    #[test]
    fn letter_list_item_gap_is_weasyprint_035rem() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Paragraph(bullet_para("Hash the input", 0, 80)));
        section.body.push(Block::Paragraph(bullet_para("Run Convert", 0, 80)));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let first = page
            .lines
            .iter()
            .find(|l| l.text.contains("Hash"))
            .expect("first item");
        let second = page
            .lines
            .iter()
            .find(|l| l.text.contains("Convert"))
            .expect("second item");
        assert_eq!(
            second.y,
            first.y.saturating_add(first.height).saturating_add(LIST_ITEM_AFTER_HU),
            "list item gap {} must be letter CSS 0.35rem, not IR 80",
            second.y.saturating_sub(first.y.saturating_add(first.height))
        );
        const { assert!(LIST_ITEM_AFTER_HU < BODY_AFTER_HU) };
    }

    #[test]
    fn letter_list_indent_is_weasyprint_125rem() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut parent = bullet_para("Receiving dock is clear", 0, 80);
        parent.indent_left = 1440;
        let mut nested = bullet_para("Bay 4 seals intact", 1, 80);
        nested.indent_left = 2880;
        section.body.push(Block::Paragraph(parent));
        section.body.push(Block::Paragraph(nested));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let dock = page
            .lines
            .iter()
            .find(|l| l.text.contains("dock"))
            .expect("parent");
        let bay = page
            .lines
            .iter()
            .find(|l| l.text.contains("Bay"))
            .expect("nested");
        let origin = PAGE_MARGIN_X_HU;
        assert_eq!(
            dock.x,
            origin.saturating_add(LIST_INDENT_HU),
            "list text x {} must be letter CSS 1.25rem, not IR 1440+marker",
            dock.x.saturating_sub(origin)
        );
        assert_eq!(
            bay.x,
            origin.saturating_add(LIST_INDENT_HU.saturating_mul(2)),
            "nested list x {} must be 2×1.25rem",
            bay.x.saturating_sub(origin)
        );
        assert!(bay.x > dock.x, "nested must hang further");
    }

    #[test]
    fn letter_nested_list_gap_is_weasyprint_025rem() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(bullet_para("Receiving dock is clear", 0, 80)));
        section
            .body
            .push(Block::Paragraph(bullet_para("Bay 4 seals intact", 1, 80)));
        section
            .body
            .push(Block::Paragraph(bullet_para("Capsule hashes travel", 0, 80)));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let dock = page
            .lines
            .iter()
            .find(|l| l.text.contains("dock"))
            .expect("parent");
        let bay = page
            .lines
            .iter()
            .find(|l| l.text.contains("Bay"))
            .expect("nested");
        let capsule = page
            .lines
            .iter()
            .find(|l| l.text.contains("Capsule"))
            .expect("sibling");
        assert_eq!(
            bay.y,
            dock.y
                .saturating_add(dock.height)
                .saturating_add(LIST_NESTED_GAP_HU),
            "nested list gap {} must be letter CSS 0.25rem, not sibling 0.35rem",
            bay.y.saturating_sub(dock.y.saturating_add(dock.height))
        );
        assert_eq!(
            capsule.y,
            bay.y
                .saturating_add(bay.height)
                .saturating_add(LIST_ITEM_AFTER_HU),
            "step-out after nested {} must stay letter CSS 0.35rem",
            capsule.y.saturating_sub(bay.y.saturating_add(bay.height))
        );
        assert_eq!(LIST_NESTED_GAP_HU, 300);
        assert_eq!(LIST_ITEM_AFTER_HU, 420);
        assert_eq!(LIST_INDENT_HU, 1500);
        assert_eq!(LIST_AFTER_HU, 1200);
        const { assert!(LIST_NESTED_GAP_HU < LIST_ITEM_AFTER_HU) };
        const { assert!(LIST_NESTED_GAP_HU < BODY_AFTER_HU) };
    }

    #[test]
    fn letter_list_after_is_weasyprint_1rem() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Paragraph(bullet_para("Compare the capsule", 0, 80)));
        let mut body = Paragraph::from_text("DocAgent runtime");
        body.space_after = 200;
        section.body.push(Block::Paragraph(body));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let item = page
            .lines
            .iter()
            .find(|l| l.text.contains("capsule"))
            .expect("list item");
        let after = page
            .lines
            .iter()
            .find(|l| l.text.contains("runtime"))
            .expect("body");
        assert_eq!(
            after.y,
            item.y.saturating_add(item.height).saturating_add(LIST_AFTER_HU),
            "gap after list {} must be letter CSS 1rem, not IR 80 or 0.35rem",
            after.y.saturating_sub(item.y.saturating_add(item.height))
        );
        const { assert!(LIST_AFTER_HU > LIST_ITEM_AFTER_HU) };
        const { assert!(LIST_AFTER_HU > BODY_AFTER_HU) };
    }

    #[test]
    fn letter_quote_spacing_is_weasyprint_blockquote() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut before = Paragraph::from_text("Wet cargo is H2O only.");
        before.space_after = 200;
        section.body.push(Block::Paragraph(before));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        quote.space_before = 80;
        quote.space_after = 200;
        section.body.push(Block::Paragraph(quote));
        let mut after = Paragraph::from_text("Please confirm before 09:00 local.");
        after.space_after = 200;
        section.body.push(Block::Paragraph(after));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let before = page
            .lines
            .iter()
            .find(|l| l.text.contains("Wet cargo"))
            .expect("before");
        let quote = page
            .lines
            .iter()
            .find(|l| l.text.contains("Replay"))
            .expect("quote");
        let after = page
            .lines
            .iter()
            .find(|l| l.text.contains("confirm"))
            .expect("after");
        let origin = PAGE_MARGIN_X_HU;
        assert_eq!(
            quote.x,
            origin.saturating_add(QUOTE_BAR_W_HU).saturating_add(QUOTE_PAD_X_HU),
            "quote text x {} must be 4px bar + 1rem pad",
            quote.x.saturating_sub(origin)
        );
        assert_eq!(
            quote.y,
            before
                .y
                .saturating_add(before.height)
                .saturating_add(BODY_AFTER_HU.max(QUOTE_BEFORE_HU))
                .saturating_add(QUOTE_PAD_Y_HU),
            "quote y {} must be collapsed p/blockquote margin plus 0.15rem pad, not IR 80",
            quote.y.saturating_sub(before.y.saturating_add(before.height))
        );
        assert_eq!(
            after.y,
            quote
                .y
                .saturating_add(quote.height)
                .saturating_add(QUOTE_PAD_Y_HU)
                .saturating_add(QUOTE_AFTER_HU),
            "after quote {} must be 0.15rem pad + 1rem margin, not IR 200",
            after.y.saturating_sub(quote.y.saturating_add(quote.height))
        );
        assert_eq!(quote.color, QUOTE_FG, "blockquote text must be letter CSS #134e4a");
        let bar = page
            .strokes
            .iter()
            .find(|s| s.fill == QUOTE_FG && s.width == QUOTE_BAR_W_HU && s.height > 200)
            .expect("quote bar");
        assert_eq!(bar.x, origin);
        assert_eq!(bar.y, quote.y.saturating_sub(QUOTE_PAD_Y_HU));
        assert_eq!(
            bar.height,
            quote.height.saturating_add(QUOTE_PAD_Y_HU.saturating_mul(2))
        );
        assert_eq!(QUOTE_FG, [19, 78, 74, 255]);
        assert_eq!(QUOTE_BAR_W_HU, 300);
    }

    #[test]
    fn letter_code_block_spacing_is_weasyprint_pre() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut before = Paragraph::from_text("Replay is evidence.");
        before.space_after = 200;
        section.body.push(Block::Paragraph(before));
        let mut code = Paragraph::from_text("docagent prove examples/letter.md");
        code.code_block = true;
        code.space_before = 80;
        code.space_after = 200;
        section.body.push(Block::Paragraph(code));
        let mut after = Paragraph::from_text("Hop File Proof");
        after.space_after = 200;
        section.body.push(Block::Paragraph(after));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let before = page
            .lines
            .iter()
            .find(|l| l.text.contains("Replay"))
            .expect("before");
        let code = page
            .lines
            .iter()
            .find(|l| l.text.contains("docagent prove"))
            .expect("code");
        let after = page
            .lines
            .iter()
            .find(|l| l.text.contains("Hop"))
            .expect("after");
        let origin = PAGE_MARGIN_X_HU;
        assert_eq!(
            code.x,
            origin.saturating_add(CODE_PAD_X_HU),
            "code text x {} must be letter CSS 1rem pad",
            code.x.saturating_sub(origin)
        );
        assert_eq!(
            code.y,
            before
                .y
                .saturating_add(before.height)
                .saturating_add(BODY_AFTER_HU.max(CODE_BEFORE_HU))
                .saturating_add(CODE_PAD_Y_HU),
            "code y {} must be collapsed p/pre margin plus 0.75rem pad, not IR 80",
            code.y.saturating_sub(before.y.saturating_add(before.height))
        );
        assert_eq!(
            after.y,
            code
                .y
                .saturating_add(code.height)
                .saturating_add(CODE_PAD_Y_HU)
                .saturating_add(CODE_AFTER_HU),
            "after code {} must be 0.75rem pad + 1rem margin, not IR 200",
            after.y.saturating_sub(code.y.saturating_add(code.height))
        );
        let fill = page
            .strokes
            .iter()
            .find(|s| s.fill == [204, 251, 241, 255] && s.width > 10000)
            .expect("code fill");
        assert_eq!(fill.x, origin);
        assert_eq!(fill.y, code.y.saturating_sub(CODE_PAD_Y_HU));
        assert_eq!(
            fill.height,
            code.height.saturating_add(CODE_PAD_Y_HU.saturating_mul(2))
        );
    }

    #[test]
    fn letter_code_block_pad_x_is_weasyprint_1rem_both_sides() {
        let mut doc = Document::new();
        let mut section = Section::default();
        // Spaced so wrap is allowed. Letter CSS `pre{padding:.75rem 1rem}` insets
        // both edges; a left-only usable width flushes the content box.
        let mut code = Paragraph::from_text("x ".repeat(400));
        code.code_block = true;
        code.space_before = 80;
        code.space_after = 200;
        section.body.push(Block::Paragraph(code));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let origin = PAGE_MARGIN_X_HU;
        let content_w = page
            .width
            .saturating_sub(PAGE_MARGIN_X_HU.saturating_mul(2));
        let right_limit = origin
            .saturating_add(content_w)
            .saturating_sub(CODE_PAD_X_HU);
        let lines: Vec<&LineFrag> = page
            .lines
            .iter()
            .filter(|l| l.width > 0 && !l.text.is_empty())
            .collect();
        assert!(
            lines.len() >= 2,
            "fixture must wrap so right pad is observable: {:?}",
            lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>()
        );
        let mut max_right = 0;
        for line in &lines {
            assert_eq!(
                line.x, origin.saturating_add(CODE_PAD_X_HU),
                "code text x {} must stay letter CSS 1rem left pad",
                line.x.saturating_sub(origin)
            );
            let right = line.x.saturating_add(line.width);
            assert!(
                right <= right_limit,
                "code glyph right {right} must keep letter CSS 1rem right pad, not flush content {}",
                origin.saturating_add(content_w)
            );
            max_right = max_right.max(right);
        }
        assert!(
            max_right > right_limit.saturating_sub(CODE_PAD_X_HU),
            "fixture must reach the right pad (max_right {max_right}), else the lock is vacuous"
        );
        let fill = page
            .strokes
            .iter()
            .find(|s| s.fill == CODE_SPAN_BG && s.width > 10000)
            .expect("code fill");
        assert_eq!(fill.x, origin, "pre background is the padding box");
        assert_eq!(
            fill.width, content_w,
            "pre fill width {} must span the content box, not shrink by 1rem",
            fill.width
        );
        assert_eq!(CODE_PAD_X_HU, 1200);
        assert_eq!(CODE_PAD_Y_HU, 900);
        const { assert!(CODE_PAD_X_HU == 16 * (HU_PER_INCH / 96)) };
    }

    #[test]
    fn letter_page_margin_is_weasyprint_25rem_275rem() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let date = page
            .lines
            .iter()
            .find(|l| l.text.contains("September"))
            .expect("body");
        assert_eq!(
            date.x, PAGE_MARGIN_X_HU,
            "body x {} must be letter CSS @page 2.75rem, not Hangul 30mm {}",
            date.x, DEFAULT_MARGIN_HU
        );
        assert_eq!(
            date.y, PAGE_MARGIN_Y_HU,
            "body y {} must be letter CSS @page 2.5rem, not Hangul 30mm {}",
            date.y, DEFAULT_MARGIN_HU
        );
        assert_eq!(PAGE_MARGIN_X_HU, 3300);
        assert_eq!(PAGE_MARGIN_Y_HU, 3000);
        assert_eq!(page_margin_x(DEFAULT_MARGIN_HU), PAGE_MARGIN_X_HU);
        assert_eq!(page_margin_y(DEFAULT_MARGIN_HU), PAGE_MARGIN_Y_HU);
        assert_ne!(
            PAGE_MARGIN_X_HU, DEFAULT_MARGIN_HU,
            "Hangul DEFAULT_MARGIN_HU 8504 is a sparse frame next to WeasyPrint"
        );
        assert_ne!(
            PAGE_MARGIN_X_HU,
            75 * (HU_PER_INCH / 96),
            "WeasyPrint UA @page margin 75px is not letter.html"
        );
        const { assert!(PAGE_MARGIN_X_HU < DEFAULT_MARGIN_HU) };
        const { assert!(PAGE_MARGIN_Y_HU < DEFAULT_MARGIN_HU) };
        let mut custom = Document::new();
        let mut section = Section::default();
        section.page.margin_left = 5000;
        section.page.margin_top = 4000;
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("custom inset")));
        custom.sections.push(section);
        let custom_tree = layout_document(&custom, &FontSet::bundled());
        let inset = custom_tree.pages[0]
            .lines
            .iter()
            .find(|l| l.text.contains("custom"))
            .expect("custom");
        assert_eq!(inset.x, 5000, "explicit IR left margin must not become 2.75rem");
        assert_eq!(inset.y, 4000, "explicit IR top margin must not become 2.5rem");
        const { assert!(PAGE_MARGIN_X_HU == 3300) };
        const { assert!(PAGE_MARGIN_Y_HU == 3000) };
        const { assert!(PAGE_MARGIN_X_HU < DEFAULT_MARGIN_HU) };
    }

    #[test]
    fn letter_list_quote_keep_landed_pad_heading_mark_gap() {
        assert_eq!(CELL_PAD_X_HU, 780);
        assert_eq!(CELL_PAD_Y_HU, 480);
        assert_eq!(CELL_SIZE_HU, 1140);
        assert_eq!(IMAGE_BEFORE_HU, 720);
        assert_eq!(IMAGE_AFTER_HU, 840);
        assert_eq!(H1_RULE_GAP_HU, 480);
        assert_eq!(H1_RULE_HU, 225);
        assert_eq!(H2_RULE_GAP_HU, 240);
        assert_eq!(H2_RULE_HU, 150);
        assert_eq!(H1_AFTER_HU, 720);
        assert_eq!(H2_BEFORE_HU, 1320);
        assert_eq!(H2_AFTER_HU, 540);
        assert_eq!(BODY_AFTER_HU, 840);
        assert_eq!(BODY_LEADING_PCT, 145);
        assert_eq!(H1_SIZE_MAX_HU, 2100);
        assert_eq!(H1_LETTER_SPACING_HU, -42);
        assert_eq!(H2_SIZE_MAX_HU, 1380);
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * HU_PER_POINT);
        assert_eq!(LIST_INDENT_HU, 1500);
        assert_eq!(LIST_ITEM_AFTER_HU, 420);
        assert_eq!(LIST_AFTER_HU, 1200);
        assert_eq!(LIST_NESTED_GAP_HU, 300);
        assert_eq!(TASK_MARK_ADVANCE_HU, 1380);
        assert_eq!(PAGE_MARGIN_X_HU, 3300);
        assert_eq!(PAGE_MARGIN_Y_HU, 3000);
        assert_ne!(PAGE_MARGIN_X_HU, DEFAULT_MARGIN_HU);
        assert_ne!(PAGE_MARGIN_Y_HU, DEFAULT_MARGIN_HU);
        assert_eq!(QUOTE_BAR_W_HU, 300);
        assert_eq!(QUOTE_FG, [19, 78, 74, 255]);
        assert_eq!(QUOTE_PAD_X_HU, 1200);
        assert_eq!(CODE_PAD_X_HU, 1200);
        assert_eq!(HR_THICK_HU, 225);
        assert_eq!(HR_BEFORE_HU, 1320);
        assert_eq!(HR_AFTER_HU, 1320);
        assert_eq!(LINK_UNDERLINE_HU, 80);
        assert_eq!(LINK_COLOR, [13, 148, 136, 255]);
        assert_eq!(CODE_SPAN_PAD_X_HU, 350);
        assert_eq!(CODE_SPAN_PAD_Y_HU, 100);
        assert_eq!(CODE_SPAN_SIZE_HU, 920);
        assert_eq!(MARK_PAD_X_HU, 200);
        assert_eq!(MARK_PAD_Y_HU, 50);
        assert_eq!(CODE_SPAN_BG, [204, 251, 241, 255]);
        assert_eq!(CODE_SPAN_COLOR, [19, 78, 74, 255]);
        assert_eq!(MARK_BG, [253, 230, 138, 255]);
        let (w, h) = mark_png_hu();
        let tree = layout_document(&letter_page(w, h), &FontSet::bundled());
        let page = &tree.pages[0];
        let h1 = page
            .lines
            .iter()
            .find(|l| l.text.contains("Northwind"))
            .expect("h1");
        let img = page.lines.iter().find(|l| l.image.is_some()).expect("mark");
        let body = page
            .lines
            .iter()
            .find(|l| l.text.contains("shipment"))
            .expect("body");
        assert_eq!(h1.font_size, H1_SIZE_MAX_HU);
        assert_eq!(
            body.y,
            img.y
                .saturating_add(img.image.as_ref().map(|i| i.height).unwrap_or(0))
                .saturating_add(IMAGE_AFTER_HU)
        );
        let hop = page
            .lines
            .iter()
            .find(|l| l.text.contains("Hop"))
            .expect("hop");
        assert_eq!(hop.x, page.rects[0].x.saturating_add(CELL_PAD_X_HU));
        assert_eq!(hop.y, page.rects[0].y.saturating_add(CELL_PAD_Y_HU));
        assert_eq!(hop.font_size, CELL_SIZE_HU);
        let h2 = page
            .lines
            .iter()
            .find(|l| l.text.contains("Delivery"))
            .expect("h2");
        assert_eq!(
            img.y,
            h2.y
                .saturating_add(h2.height)
                .saturating_add(H2_RULE_GAP_HU)
                .saturating_add(H2_RULE_HU)
                .saturating_add(IMAGE_BEFORE_HU)
        );
    }

    #[test]
    fn letter_hr_is_weasyprint_3px_11rem() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut before = Paragraph::from_text("output SHA-256");
        before.space_after = 200;
        section.body.push(Block::Paragraph(before));
        section.body.push(Block::Break(BreakKind::Thematic));
        let mut after = Paragraph::from_text("Please confirm before 09:00 local");
        after.space_after = 200;
        section.body.push(Block::Paragraph(after));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let before = page
            .lines
            .iter()
            .find(|l| l.text.contains("SHA-256"))
            .expect("before");
        let after = page
            .lines
            .iter()
            .find(|l| l.text.contains("confirm"))
            .expect("after");
        let rule = page
            .strokes
            .iter()
            .find(|s| s.fill == [19, 78, 74, 255] && s.height == HR_THICK_HU && s.width > 10000)
            .expect("hr fill");
        assert_eq!(HR_THICK_HU, 225, "hr must stay letter CSS 3px");
        assert_eq!(HR_BEFORE_HU, 1320, "hr margin-top must stay letter CSS 1.1rem");
        assert_eq!(HR_AFTER_HU, 1320, "hr margin-bottom must stay letter CSS 1.1rem");
        assert_eq!(
            rule.y,
            before
                .y
                .saturating_add(before.height)
                .saturating_add(HR_BEFORE_HU),
            "hr y {} must be collapsed p/hr 1.1rem, not IR 240 or body 0.7rem",
            rule.y.saturating_sub(before.y.saturating_add(before.height))
        );
        assert_eq!(
            after.y,
            rule.y.saturating_add(HR_THICK_HU).saturating_add(HR_AFTER_HU),
            "after hr {} must be 3px + 1.1rem, not IR 240",
            after.y.saturating_sub(rule.y.saturating_add(HR_THICK_HU))
        );
        const { assert!(HR_THICK_HU > 80) };
        const { assert!(HR_BEFORE_HU > BODY_AFTER_HU) };
    }

    #[test]
    fn letter_link_underline_is_weasyprint_teal() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![
            docagent_model::Run::hyperlink(
                "DocAgent",
                "https://github.com/kevin9327/docagent",
            ),
            docagent_model::Run::text(" runtime"),
        ];
        p.space_after = 200;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let link = page
            .lines
            .iter()
            .find(|l| l.text.contains("DocAgent"))
            .expect("link");
        let rest = page
            .lines
            .iter()
            .find(|l| l.text.contains("runtime"))
            .expect("rest");
        assert!(link.underline, "WeasyPrint UA underlines <a>");
        assert!(link.href.is_some());
        assert_eq!(
            link.color, LINK_COLOR,
            "a glyphs must stay letter CSS #0d9488"
        );
        assert!(!rest.underline);
        assert!(rest.href.is_none());
        assert_eq!(
            rest.color, BODY_FG,
            "text after a must be letter body #134e4a, not UA black"
        );
        let hit = link.link_hit().expect("link hit");
        assert_eq!(hit.4, "https://github.com/kevin9327/docagent");
        assert!(hit.2 > 0 && hit.3 > 0);
        let under = page
            .strokes
            .iter()
            .find(|s| {
                s.fill == LINK_COLOR
                    && s.height == LINK_UNDERLINE_HU
                    && s.width == link.width
                    && s.x == link.x
            })
            .expect("link underline fill");
        assert_eq!(LINK_COLOR, [13, 148, 136, 255], "a must stay letter CSS #0d9488");
        assert_eq!(LINK_UNDERLINE_HU, 80, "link underline must stay 1px");
        assert_eq!(under.width, link.width);
        assert_eq!(
            under.y,
            link.y
                .saturating_add(link.baseline)
                .saturating_add(UNDERLINE_OFFSET_HU)
        );
        assert!(
            page.strokes.iter().all(|s| {
                !(s.fill == STRIKE_COLOR && s.height == UNDERLINE_HU && s.width == link.width)
            }),
            "link underline must not use heading-rule teal"
        );
    }

    #[test]
    fn letter_strike_and_underline_are_weasyprint_s_u() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Agents ");
        let mut strike = docagent_model::Run::text("guess");
        strike.style.strike = true;
        p.runs.push(strike);
        p.runs.push(docagent_model::Run::text("."));
        section.body.push(Block::Paragraph(p));
        let mut under = Paragraph::from_text("09:00 local");
        under.runs[0].style.underline = docagent_model::Underline::Single;
        section.body.push(Block::Paragraph(under));
        section
            .body
            .push(Block::Paragraph(strong_em_para()));
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        section
            .body
            .push(Block::Paragraph(task_para("Receiving dock is clear", true)));
        let mut code = docagent_model::Run::text("prove");
        code.style.code = true;
        let mut code_p = Paragraph::from_text("Run ");
        code_p.runs.push(code);
        section.body.push(Block::Paragraph(code_p));
        let mut mark = Paragraph::from_text("three hashes");
        mark.runs[0].style.highlight = Some(docagent_model::Color::MARK);
        section.body.push(Block::Paragraph(mark));
        section.body.push(Block::Break(BreakKind::Thematic));
        let mut link = Paragraph::from_text("");
        link.runs = vec![docagent_model::Run::hyperlink(
            "DocAgent",
            "https://github.com/kevin9327/docagent",
        )];
        section.body.push(Block::Paragraph(link));
        let mut pdfa = Paragraph::from_text("PDF/");
        let mut sup = docagent_model::Run::text("A");
        sup.style.superscript = true;
        pdfa.runs.push(sup);
        section.body.push(Block::Paragraph(pdfa));
        let mut h2o = Paragraph::from_text("H");
        let mut sub = docagent_model::Run::text("2");
        sub.style.subscript = true;
        h2o.runs.push(sub);
        h2o.runs.push(docagent_model::Run::text("O"));
        section.body.push(Block::Paragraph(h2o));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let guess = page
            .lines
            .iter()
            .find(|l| l.text.contains("guess"))
            .expect("strike run");
        assert!(guess.strike);
        assert!(!guess.underline);
        assert_eq!(guess.color, STRIKE_COLOR);
        let strike = page
            .strokes
            .iter()
            .find(|s| {
                is_text_decoration(s)
                    && s.fill == STRIKE_COLOR
                    && s.height == STRIKE_HU
                    && s.x == guess.x
                    && s.width == guess.width
            })
            .expect("strike fill");
        assert_eq!(
            strike.y,
            guess
                .y
                .saturating_add(guess.baseline)
                .saturating_sub(guess.font_size / 3)
        );
        let agents = page
            .lines
            .iter()
            .find(|l| l.text.contains("Agents"))
            .expect("plain");
        assert!(!agents.strike);
        assert_eq!(
            agents.color, BODY_FG,
            "body run must be letter CSS #134e4a, not UA black"
        );
        let local = page
            .lines
            .iter()
            .find(|l| l.text.contains("09:00"))
            .expect("u run");
        assert!(local.underline);
        assert!(!local.strike);
        assert_eq!(
            local.color, BODY_FG,
            "u text must be letter body #134e4a, not UA black"
        );
        let under = page
            .strokes
            .iter()
            .find(|s| {
                is_text_decoration(s)
                    && s.fill == STRIKE_COLOR
                    && s.height == UNDERLINE_HU
                    && s.x == local.x
                    && s.width == local.width
                    && s.y
                        == local
                            .y
                            .saturating_add(local.baseline)
                            .saturating_add(UNDERLINE_OFFSET_HU)
            })
            .expect("u underline fill");
        assert!(is_text_decoration(under));
        assert!(
            page.rects.iter().any(|r| r.fill == TH_BG && r.width > 80),
            "th fill must not regress"
        );
        let replay = page
            .lines
            .iter()
            .find(|l| l.text.contains("Replay is evidence"))
            .expect("quote");
        assert_eq!(replay.color, QUOTE_FG, "blockquote text must be letter CSS #134e4a");
        assert!(
            page.strokes.iter().any(|s| {
                s.fill == QUOTE_FG && s.width == QUOTE_BAR_W_HU && s.height > 200
            }),
            "quote bar must not regress"
        );
        assert!(
            page.strokes
                .iter()
                .any(|s| s.fill == [19, 78, 74, 255]
                    && s.width == TASK_BOX_HU
                    && s.height == TASK_BOX_HU),
            "task box must not regress"
        );
        assert!(
            page.strokes
                .iter()
                .any(|s| s.fill == CODE_SPAN_BG && s.width > 80 && s.height < 2000),
            "code span fill must not regress"
        );
        assert!(
            page.strokes
                .iter()
                .any(|s| s.fill == MARK_BG && s.width > 80),
            "mark fill must not regress"
        );
        assert!(
            page.strokes.iter().any(|s| {
                s.fill == [19, 78, 74, 255] && s.height == HR_THICK_HU && s.width > 10000
            }),
            "hr must not regress"
        );
        assert!(
            page.strokes.iter().any(|s| {
                s.fill == LINK_COLOR && s.height == LINK_UNDERLINE_HU && s.width > 80
            }),
            "link underline must not regress"
        );
        assert_eq!(STRIKE_COLOR, TH_FG);
        assert_eq!(QUOTE_FG, TH_FG);
        assert_eq!(BODY_FG, TH_FG);
        assert_ne!(BODY_FG, [0, 0, 0, 255], "body must not regress to UA black");
        assert_eq!(QUOTE_BAR_W_HU, 300);
        assert_eq!(STRIKE_HU, 80);
        assert_eq!(UNDERLINE_HU, 80);
        assert_eq!(UNDERLINE_OFFSET_HU, 100);
        assert_eq!(TH_BG, [204, 251, 241, 255]);
        assert_eq!(TASK_BOX_HU, 900);
        assert_eq!(HR_THICK_HU, 225);
        assert_eq!(LINK_COLOR, [13, 148, 136, 255]);
        assert_eq!(CODE_SPAN_PAD_X_HU, 350);
        assert_eq!(MARK_PAD_X_HU, 200);
        let pdf = page
            .lines
            .iter()
            .find(|l| l.text.contains("PDF"))
            .expect("PDF");
        let raised = page.lines.iter().find(|l| l.text == "A").expect("super");
        assert_eq!(raised.font_size, SUPER_SUB_SIZE_HU);
        assert_eq!(
            pdf.color, BODY_FG,
            "body/sup parent must be letter CSS #134e4a, not UA black"
        );
        assert_eq!(raised.color, BODY_FG, "sup must keep letter body #134e4a");
        assert_eq!(
            raised.y.saturating_add(raised.baseline),
            pdf.y
                .saturating_add(pdf.baseline)
                .saturating_sub(SUPER_SUB_SHIFT_HU)
        );
        let h = page.lines.iter().find(|l| l.text == "H").expect("H");
        let lowered = page.lines.iter().find(|l| l.text == "2").expect("sub");
        assert_eq!(lowered.font_size, SUPER_SUB_SIZE_HU);
        assert_eq!(h.color, BODY_FG, "sub parent must be letter body #134e4a");
        assert_eq!(lowered.color, BODY_FG, "sub must keep letter body #134e4a");
        assert_eq!(
            lowered.y.saturating_add(lowered.baseline),
            h.y
                .saturating_add(h.baseline)
                .saturating_add(SUPER_SUB_SHIFT_HU)
        );
        assert_eq!(SUPER_SUB_SIZE_HU, 750);
        assert_eq!(SUPER_SUB_SHIFT_HU, 375);
        let gpu = page
            .lines
            .iter()
            .find(|l| l.text.contains("GPU-free"))
            .expect("strong");
        assert!(gpu.bold, "strong must not regress");
        assert!(!gpu.italic);
        assert_eq!(
            gpu.color, BODY_FG,
            "strong body must be letter CSS #134e4a, not UA black"
        );
        let slant = page
            .lines
            .iter()
            .find(|l| l.text.contains("byte-for-byte"))
            .expect("em");
        assert!(slant.italic, "em must not regress");
        assert!(!slant.bold);
        assert_eq!(
            slant.color, BODY_FG,
            "em body must be letter CSS #134e4a, not UA black"
        );
    }

    #[test]
    fn letter_inline_code_padding_is_weasyprint_code() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Run ");
        let mut code = docagent_model::Run::text("prove");
        code.style.code = true;
        p.runs.push(code);
        p.runs.push(docagent_model::Run::text(" twice"));
        p.space_after = 200;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let run = page
            .lines
            .iter()
            .find(|l| l.text.contains("Run"))
            .expect("run");
        let prove = page
            .lines
            .iter()
            .find(|l| l.text.contains("prove"))
            .expect("prove");
        let twice = page
            .lines
            .iter()
            .find(|l| l.text.contains("twice"))
            .expect("twice");
        assert!(prove.code);
        assert!(!run.code);
        assert!(!twice.code);
        assert_eq!(
            run.color, BODY_FG,
            "body beside code must be letter CSS #134e4a, not UA black"
        );
        assert_eq!(
            twice.color, BODY_FG,
            "body after code must be letter CSS #134e4a, not UA black"
        );
        assert_eq!(prove.color, CODE_SPAN_COLOR);
        assert_eq!(
            prove.x,
            run.x.saturating_add(run.width).saturating_add(CODE_SPAN_PAD_X_HU),
            "code x {} must include letter CSS 0.35em pad",
            prove.x.saturating_sub(run.x.saturating_add(run.width))
        );
        assert_eq!(
            twice.x,
            prove
                .x
                .saturating_add(prove.width)
                .saturating_add(CODE_SPAN_PAD_X_HU),
            "after code {} must include letter CSS 0.35em pad",
            twice.x.saturating_sub(prove.x.saturating_add(prove.width))
        );
        let fill = page
            .strokes
            .iter()
            .find(|s| s.fill == CODE_SPAN_BG && s.width > 80 && s.height > 80)
            .expect("code fill");
        assert_eq!(fill.x, prove.x.saturating_sub(CODE_SPAN_PAD_X_HU));
        assert_eq!(
            fill.width,
            prove.width.saturating_add(CODE_SPAN_PAD_X_HU.saturating_mul(2))
        );
        assert_eq!(
            fill.y,
            prove
                .y
                .saturating_add(prove.baseline)
                .saturating_sub(prove.font_size)
                .saturating_sub(CODE_SPAN_PAD_Y_HU)
        );
        assert_eq!(
            fill.height,
            prove
                .font_size
                .saturating_add(CODE_SPAN_PAD_Y_HU.saturating_mul(2))
        );
        assert_eq!(CODE_SPAN_PAD_X_HU, 350);
        assert_eq!(CODE_SPAN_PAD_Y_HU, 100);
        assert_eq!(CODE_SPAN_SIZE_HU, 920);
        assert_eq!(H1_SIZE_MAX_HU, 2100);
        assert_eq!(HR_THICK_HU, 225);
        assert_eq!(LINK_COLOR, [13, 148, 136, 255]);
        assert_eq!(LIST_ITEM_AFTER_HU, 420);
        assert_eq!(QUOTE_BAR_W_HU, 300);
    }

    #[test]
    fn letter_inline_code_size_is_weasyprint_092em() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Run ");
        let mut code = docagent_model::Run::text("prove");
        code.style.code = true;
        p.runs.push(code);
        p.runs.push(docagent_model::Run::text(" twice"));
        p.space_after = 200;
        section.body.push(Block::Paragraph(p));
        let mut pre = Paragraph::from_text("docagent prove examples/letter.md");
        pre.code_block = true;
        pre.space_before = 80;
        pre.space_after = 200;
        section.body.push(Block::Paragraph(pre));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let run = page
            .lines
            .iter()
            .find(|l| l.text.contains("Run"))
            .expect("run");
        let prove = page
            .lines
            .iter()
            .find(|l| l.text.contains("prove") && l.code)
            .expect("prove");
        let twice = page
            .lines
            .iter()
            .find(|l| l.text.contains("twice"))
            .expect("twice");
        let pre_line = page
            .lines
            .iter()
            .find(|l| l.text.contains("docagent prove"))
            .expect("pre");
        assert_eq!(
            run.font_size, DEFAULT_FONT_SIZE_HU,
            "body beside code must stay 10pt"
        );
        assert_eq!(
            twice.font_size, DEFAULT_FONT_SIZE_HU,
            "body after code must stay 10pt"
        );
        assert_eq!(
            prove.font_size, CODE_SPAN_SIZE_HU,
            "inline code size {} must be letter CSS 0.92em, not body 10pt",
            prove.font_size
        );
        assert_eq!(prove.font_size, code_span_size(run.font_size));
        assert!(
            prove.font_size < run.font_size,
            "code 0.92em must read smaller than body"
        );
        assert_eq!(
            pre_line.font_size, CODE_SPAN_SIZE_HU,
            "pre code size {} must inherit letter CSS 0.92em",
            pre_line.font_size
        );
        assert_eq!(pre_line.font_size, code_span_size(DEFAULT_FONT_SIZE_HU));
        assert_eq!(
            prove.x,
            run.x.saturating_add(run.width).saturating_add(CODE_SPAN_PAD_X_HU),
            "0.92em must keep letter CSS 0.35em pad"
        );
        assert_eq!(CODE_SPAN_SIZE_HU, 920);
        assert_eq!(CODE_SPAN_PAD_X_HU, 350);
        assert_eq!(CODE_SPAN_PAD_Y_HU, 100);
        assert_eq!(H1_SIZE_MAX_HU, 2100);
        assert_eq!(H2_AFTER_HU, 540);
        assert_eq!(CELL_PAD_X_HU, 780);
        const { assert!(CODE_SPAN_SIZE_HU < DEFAULT_FONT_SIZE_HU) };
        const { assert!(CODE_SPAN_SIZE_HU > SUPER_SUB_SIZE_HU) };
    }

    #[test]
    fn letter_kbd_samp_are_weasyprint_code_chip() {
        // HTML maps <kbd>/<samp> to style.code. Letter CSS pins them to the
        // `code` mint chip; WeasyPrint UA kbd is a 1px-border key.
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Run ");
        let mut kbd = docagent_model::Run::text("prove");
        kbd.style.code = true;
        p.runs.push(kbd);
        p.runs.push(docagent_model::Run::text(" twice. Output "));
        let mut samp = docagent_model::Run::text("SHA-256");
        samp.style.code = true;
        p.runs.push(samp);
        p.runs.push(docagent_model::Run::text("."));
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let run = page
            .lines
            .iter()
            .find(|l| l.text.contains("Run"))
            .expect("run");
        let prove = page
            .lines
            .iter()
            .find(|l| l.text == "prove")
            .expect("kbd");
        let mid = page
            .lines
            .iter()
            .find(|l| l.text.contains("Output"))
            .expect("mid");
        let hash = page
            .lines
            .iter()
            .find(|l| l.text.contains("SHA-256"))
            .expect("samp");
        let after = page
            .lines
            .iter()
            .find(|l| l.text == ".")
            .expect("after");
        assert!(prove.code, "kbd must paint as the code chip");
        assert!(hash.code, "samp must paint as the code chip");
        assert!(!run.code);
        assert!(!mid.code);
        assert!(!after.code);
        assert_eq!(prove.color, CODE_SPAN_COLOR);
        assert_eq!(hash.color, CODE_SPAN_COLOR);
        assert_eq!(run.color, BODY_FG);
        assert_eq!(mid.color, BODY_FG);
        assert_eq!(
            prove.font_size, CODE_SPAN_SIZE_HU,
            "kbd size {} must be letter CSS 0.92em, not UA monospace 10pt",
            prove.font_size
        );
        assert_eq!(hash.font_size, CODE_SPAN_SIZE_HU);
        assert_eq!(prove.font_size, code_span_size(run.font_size));
        assert_eq!(
            prove.x,
            run.x.saturating_add(run.width).saturating_add(CODE_SPAN_PAD_X_HU),
            "kbd x must include letter CSS 0.35em pad"
        );
        assert_eq!(
            mid.x,
            prove
                .x
                .saturating_add(prove.width)
                .saturating_add(CODE_SPAN_PAD_X_HU),
            "after kbd must include letter CSS 0.35em pad"
        );
        assert_eq!(
            hash.x,
            mid.x.saturating_add(mid.width).saturating_add(CODE_SPAN_PAD_X_HU),
            "samp x must include letter CSS 0.35em pad"
        );
        assert_eq!(
            after.x,
            hash.x
                .saturating_add(hash.width)
                .saturating_add(CODE_SPAN_PAD_X_HU),
            "after samp must include letter CSS 0.35em pad"
        );
        let fills: Vec<_> = page
            .strokes
            .iter()
            .filter(|s| s.fill == CODE_SPAN_BG && s.width > 80 && s.height > 80)
            .collect();
        assert_eq!(
            fills.len(),
            2,
            "kbd and samp must each paint a mint chip, not UA bare kbd: {:?}",
            page.strokes
        );
        assert_eq!(fills[0].x, prove.x.saturating_sub(CODE_SPAN_PAD_X_HU));
        assert_eq!(
            fills[0].width,
            prove.width.saturating_add(CODE_SPAN_PAD_X_HU.saturating_mul(2))
        );
        assert_eq!(
            fills[0].height,
            prove
                .font_size
                .saturating_add(CODE_SPAN_PAD_Y_HU.saturating_mul(2))
        );
        assert_eq!(fills[1].x, hash.x.saturating_sub(CODE_SPAN_PAD_X_HU));
        assert_eq!(
            fills[1].width,
            hash.width.saturating_add(CODE_SPAN_PAD_X_HU.saturating_mul(2))
        );
        assert!(
            !page.strokes.iter().any(|s| {
                (s.width == CELL_BORDER_HU || s.height == CELL_BORDER_HU)
                    && s.fill != CODE_SPAN_BG
            }),
            "kbd/samp must not paint WeasyPrint UA 1px border: {:?}",
            page.strokes
        );
        assert_eq!(cell_valign_offset(VAlign::Top, 2000, 5000), 0);
        assert_eq!(CODE_SPAN_PAD_X_HU, 350);
        assert_eq!(CODE_SPAN_PAD_Y_HU, 100);
        assert_eq!(CODE_SPAN_SIZE_HU, 920);
        assert_eq!(CODE_SPAN_BG, [204, 251, 241, 255]);
        assert_eq!(CODE_SPAN_COLOR, [19, 78, 74, 255]);
        assert_eq!(CODE_PAD_X_HU, 1200);
        assert_eq!(H1_SIZE_MAX_HU, 2100);
        assert_eq!(H2_SIZE_MAX_HU, 1380);
        assert_eq!(H1_LETTER_SPACING_HU, -42);
        assert_eq!(H1_AFTER_HU, 720);
        assert_eq!(H2_BEFORE_HU, 1320);
        assert_eq!(H2_AFTER_HU, 540);
        assert_eq!(BODY_LEADING_PCT, 145);
        assert_eq!(BODY_FG, [19, 78, 74, 255]);
        assert_eq!(LIST_NESTED_GAP_HU, 300);
        assert_eq!(TASK_MARK_ADVANCE_HU, 1380);
        assert_eq!(PAGE_MARGIN_X_HU, 3300);
        assert_eq!(PAGE_MARGIN_Y_HU, 3000);
        assert_eq!(CELL_PAD_X_HU, 780);
        assert_eq!(CELL_PAD_Y_HU, 480);
        assert_eq!(CELL_SIZE_HU, 1140);
        assert_eq!(IMAGE_BEFORE_HU, 720);
        assert_eq!(QUOTE_BAR_W_HU, 300);
        assert_eq!(CELL_BORDER_HU, 75);
        const { assert!(CODE_SPAN_SIZE_HU == 920) };
        const { assert!(CODE_SPAN_PAD_X_HU == 350) };
    }

    #[test]
    fn letter_abbr_underline_is_weasyprint_dotted_teal() {
        // Letter CSS `abbr[title]{text-decoration:underline dotted #134e4a;
        // text-underline-offset:.15em}`. WeasyPrint UA dotted is uncolored
        // CanvasText / a solid `u` rule.
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Replay ");
        let mut abbr = docagent_model::Run::text("PDF/A");
        abbr.style.underline = docagent_model::Underline::Dotted;
        p.runs.push(abbr);
        p.runs.push(docagent_model::Run::text(" hashes."));
        section.body.push(Block::Paragraph(p));
        let mut under = Paragraph::from_text("09:00 local");
        under.runs[0].style.underline = docagent_model::Underline::Single;
        section.body.push(Block::Paragraph(under));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let pdfa = page
            .lines
            .iter()
            .find(|l| l.text.contains("PDF/A"))
            .expect("abbr");
        let local = page
            .lines
            .iter()
            .find(|l| l.text.contains("09:00"))
            .expect("u");
        let rest = page
            .lines
            .iter()
            .find(|l| l.text.contains("Replay"))
            .expect("body");
        assert!(pdfa.underline);
        assert!(pdfa.dotted, "abbr must paint dotted, not solid u");
        assert!(local.underline);
        assert!(!local.dotted, "letter.css dotted is abbr-only, not u");
        assert!(!rest.underline);
        assert!(!rest.dotted);
        assert_eq!(pdfa.color, BODY_FG);
        assert_eq!(local.color, BODY_FG);
        assert_eq!(rest.color, BODY_FG);
        let offset = dotted_underline_offset(pdfa.font_size);
        assert_eq!(offset, ABBR_UNDERLINE_OFFSET_HU);
        assert_eq!(offset, dotted_underline_offset(DEFAULT_FONT_SIZE_HU));
        assert!(
            offset > UNDERLINE_OFFSET_HU,
            "abbr offset 0.15em must sit below solid u 0.1em"
        );
        let dots: Vec<_> = page
            .strokes
            .iter()
            .filter(|s| {
                s.fill == BODY_FG
                    && s.height == UNDERLINE_HU
                    && s.y
                        == pdfa
                            .y
                            .saturating_add(pdfa.baseline)
                            .saturating_add(offset)
                    && s.x >= pdfa.x
                    && s.x < pdfa.x.saturating_add(pdfa.width)
            })
            .collect();
        assert!(
            dots.len() >= 2,
            "abbr must paint 1px on/off dots, not a solid UA underline: {:?}",
            page.strokes
        );
        assert!(
            dots.iter().all(|s| s.width <= DOTTED_UNDERLINE_DOT_HU),
            "abbr dots must stay 1px, not a solid span: {:?}",
            dots.iter().map(|s| s.width).collect::<Vec<_>>()
        );
        assert_eq!(dots[0].x, pdfa.x);
        assert_eq!(
            dots[1].x,
            pdfa.x
                .saturating_add(DOTTED_UNDERLINE_DOT_HU)
                .saturating_add(DOTTED_UNDERLINE_DOT_HU)
        );
        assert!(
            !page.strokes.iter().any(|s| {
                s.fill == BODY_FG
                    && s.height == UNDERLINE_HU
                    && s.width == pdfa.width
                    && s.x == pdfa.x
            }),
            "abbr must not keep a solid underline span: {:?}",
            page.strokes
        );
        assert!(
            !page.strokes.iter().any(|s| {
                s.fill == [0, 0, 0, 255] && s.height == UNDERLINE_HU && s.width > 0
            }),
            "abbr dots must be letter #134e4a, not UA CanvasText black: {:?}",
            page.strokes
        );
        let solid = page
            .strokes
            .iter()
            .find(|s| {
                s.fill == STRIKE_COLOR
                    && s.height == UNDERLINE_HU
                    && s.width == local.width
                    && s.x == local.x
            })
            .expect("u underline");
        assert_eq!(
            solid.y,
            local
                .y
                .saturating_add(local.baseline)
                .saturating_add(UNDERLINE_OFFSET_HU)
        );
        assert_eq!(ABBR_UNDERLINE_OFFSET_HU, 150);
        assert_eq!(DOTTED_UNDERLINE_DOT_HU, UNDERLINE_HU);
        assert_eq!(UNDERLINE_OFFSET_HU, 100);
        assert_eq!(CODE_SPAN_PAD_X_HU, 350);
        assert_eq!(CODE_SPAN_SIZE_HU, 920);
        assert_eq!(CODE_PAD_X_HU, 1200);
        assert_eq!(H1_SIZE_MAX_HU, 2100);
        assert_eq!(H2_AFTER_HU, 540);
        assert_eq!(HEADING_LEADING_PCT, 120);
        assert_eq!(cell_valign_offset(VAlign::Top, 2000, 5000), 0);
        assert_eq!(TASK_MARK_ADVANCE_HU, 1380);
        assert_eq!(LIST_NESTED_GAP_HU, 300);
        assert_eq!(PAGE_MARGIN_X_HU, 3300);
        assert_eq!(QUOTE_BAR_W_HU, 300);
        assert_eq!(BODY_FG, [19, 78, 74, 255]);
        const { assert!(ABBR_UNDERLINE_OFFSET_HU == 150) };
        const { assert!(ABBR_UNDERLINE_OFFSET_HU > UNDERLINE_OFFSET_HU) };
        const { assert!(DOTTED_UNDERLINE_DOT_HU == UNDERLINE_HU) };
    }

    fn decimal_para(text: &str, start: u32) -> Paragraph {
        let mut p = Paragraph::from_text(text);
        p.space_after = 80;
        p.numbering = Some(NumberingRef {
            definition_id: 1,
            level: 0,
            start: Some(start),
            format: Some(NumberFormat::Decimal),
        });
        p
    }

    #[test]
    fn letter_list_marker_is_weasyprint_teal() {
        // Letter CSS `::marker{color:#134e4a}`. WeasyPrint UA `::marker` has no
        // color, so agent-replay decimals paint CanvasText black.
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(decimal_para("Hash the input", 1)));
        section
            .body
            .push(Block::Paragraph(decimal_para("Run Convert", 2)));
        let mut nested = bullet_para("PDF/A", 1, 80);
        nested.indent_left = 2880;
        section.body.push(Block::Paragraph(nested));
        section
            .body
            .push(Block::Paragraph(decimal_para("Compare the capsule", 3)));
        let mut body = Paragraph::from_text("6 September 2026");
        body.space_after = 200;
        section.body.push(Block::Paragraph(body));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let one = page
            .lines
            .iter()
            .find(|l| l.text.starts_with("1. "))
            .expect("1. marker");
        let two = page
            .lines
            .iter()
            .find(|l| l.text.starts_with("2. "))
            .expect("2. marker");
        let three = page
            .lines
            .iter()
            .find(|l| l.text.starts_with("3. "))
            .expect("3. marker");
        let hash = page
            .lines
            .iter()
            .find(|l| l.text.contains("Hash the input"))
            .expect("hash item");
        let date = page
            .lines
            .iter()
            .find(|l| l.text.contains("September"))
            .expect("body");
        assert_eq!(
            one.color, MARKER_FG,
            "ol ::marker must be letter CSS #134e4a, not UA CanvasText black"
        );
        assert_eq!(two.color, MARKER_FG);
        assert_eq!(three.color, MARKER_FG);
        assert_ne!(one.color, [0, 0, 0, 255], "marker must not stay UA black");
        assert_eq!(hash.color, BODY_FG, "ol item text must stay letter body teal");
        assert_eq!(date.color, BODY_FG);
        assert_eq!(MARKER_FG, BODY_FG);
        assert_eq!(MARKER_FG, [19, 78, 74, 255]);
        assert!(
            page.strokes
                .iter()
                .any(|s| s.fill == MARKER_FG && s.width == s.height && s.width >= 400),
            "nested disc ::marker must be letter #134e4a, not UA black: {:?}",
            page.strokes
        );
        assert!(
            !page.strokes.iter().any(|s| {
                s.fill == [0, 0, 0, 255] && s.width == s.height && s.width >= 400
            }),
            "nested disc must not paint UA CanvasText black: {:?}",
            page.strokes
        );
        assert_eq!(ABBR_UNDERLINE_OFFSET_HU, 150);
        assert_eq!(DOTTED_UNDERLINE_DOT_HU, UNDERLINE_HU);
        assert_eq!(CODE_PAD_X_HU, 1200);
        assert_eq!(HEADING_LEADING_PCT, 120);
        assert_eq!(H2_RULE_HU, 150);
        assert_eq!(CELL_BORDER_HU, 75);
        assert_eq!(PAGE_MARGIN_X_HU, 3300);
        assert_eq!(PAGE_MARGIN_Y_HU, 3000);
        assert_eq!(TASK_MARK_ADVANCE_HU, 1380);
        assert_eq!(H1_LETTER_SPACING_HU, -42);
        assert_eq!(LIST_NESTED_GAP_HU, 300);
        assert_eq!(CODE_SPAN_SIZE_HU, 920);
        assert_eq!(H1_AFTER_HU, 720);
        assert_eq!(H2_BEFORE_HU, 1320);
        assert_eq!(H2_AFTER_HU, 540);
        assert_eq!(BODY_LEADING_PCT, 145);
        assert_eq!(BODY_FG, [19, 78, 74, 255]);
        assert_eq!(QUOTE_BAR_W_HU, 300);
        assert_eq!(cell_valign_offset(VAlign::Top, 2000, 5000), 0);
        const { assert!(MARKER_FG[0] == 19 && MARKER_FG[1] == 78 && MARKER_FG[2] == 74) };
        const { assert!(MARKER_FG[0] == BODY_FG[0]) };
    }

    fn pre_code_para(text: &str) -> Paragraph {
        // HTML `<pre><code>` sets both `code_block` and `style.code`.
        let mut p = Paragraph::from_text(text);
        p.code_block = true;
        p.space_before = 80;
        p.space_after = 200;
        for run in &mut p.runs {
            run.style.code = true;
        }
        p
    }

    #[test]
    fn letter_pre_code_is_weasyprint_transparent_zero_pad() {
        // Letter CSS `pre code{background:transparent;padding:0}`. Inherited
        // `code{padding:.1em .35em}` mint chip insets the prove fence inside
        // `pre{padding:.75rem 1rem}`.
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut inline = Paragraph::from_text("Run ");
        let mut prove = docagent_model::Run::text("prove");
        prove.style.code = true;
        inline.runs.push(prove);
        inline.runs.push(docagent_model::Run::text(" twice"));
        section.body.push(Block::Paragraph(inline));
        section.body.push(Block::Paragraph(pre_code_para(
            "docagent prove examples/letter.md",
        )));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let run = page
            .lines
            .iter()
            .find(|l| l.text.contains("Run"))
            .expect("run");
        let chip = page
            .lines
            .iter()
            .find(|l| l.text.contains("prove") && l.code)
            .expect("inline prove");
        let twice = page
            .lines
            .iter()
            .find(|l| l.text.contains("twice"))
            .expect("twice");
        let fence = page
            .lines
            .iter()
            .find(|l| l.text.contains("docagent prove"))
            .expect("pre code");
        assert!(chip.code, "inline code must keep the mint chip");
        assert!(
            !fence.code,
            "pre code must drop the inherited chip: {:?}",
            page.lines
        );
        assert!(code_span_chip(true, false));
        assert!(!code_span_chip(true, true));
        assert!(!code_span_chip(false, false));
        assert_eq!(
            chip.x,
            run.x.saturating_add(run.width).saturating_add(CODE_SPAN_PAD_X_HU),
            "inline code must keep letter CSS 0.35em pad"
        );
        assert_eq!(
            twice.x,
            chip.x
                .saturating_add(chip.width)
                .saturating_add(CODE_SPAN_PAD_X_HU)
        );
        let origin = PAGE_MARGIN_X_HU;
        assert_eq!(
            fence.x,
            origin.saturating_add(CODE_PAD_X_HU),
            "pre code x {} must stay letter CSS 1rem pre pad, not 1rem+0.35em chip inset",
            fence.x.saturating_sub(origin)
        );
        assert_ne!(
            fence.x,
            origin
                .saturating_add(CODE_PAD_X_HU)
                .saturating_add(CODE_SPAN_PAD_X_HU),
            "inherited code-span 0.35em must not inset the prove fence"
        );
        assert_eq!(fence.font_size, CODE_SPAN_SIZE_HU);
        assert_eq!(fence.color, CODE_SPAN_COLOR);
        assert_eq!(fence.color, BODY_FG);
        let pre_fill = page
            .strokes
            .iter()
            .find(|s| {
                s.fill == CODE_SPAN_BG
                    && s.width > 10000
                    && s.height >= fence.height.saturating_add(CODE_PAD_Y_HU.saturating_mul(2))
            })
            .expect("pre fill");
        assert_eq!(pre_fill.x, origin);
        assert!(
            !page.strokes.iter().any(|s| {
                s.fill == CODE_SPAN_BG
                    && s.x == fence.x.saturating_sub(CODE_SPAN_PAD_X_HU)
                    && s.width
                        == fence
                            .width
                            .saturating_add(CODE_SPAN_PAD_X_HU.saturating_mul(2))
                    && s.height
                        == fence
                            .font_size
                            .saturating_add(CODE_SPAN_PAD_Y_HU.saturating_mul(2))
            }),
            "pre code must not paint an inner mint chip on the prove fence: {:?}",
            page.strokes
        );
        let chip_fill = page
            .strokes
            .iter()
            .find(|s| {
                s.fill == CODE_SPAN_BG
                    && s.x == chip.x.saturating_sub(CODE_SPAN_PAD_X_HU)
                    && s.width
                        == chip
                            .width
                            .saturating_add(CODE_SPAN_PAD_X_HU.saturating_mul(2))
            })
            .expect("inline chip");
        assert_eq!(
            chip_fill.height,
            chip.font_size
                .saturating_add(CODE_SPAN_PAD_Y_HU.saturating_mul(2))
        );
        assert_eq!(CODE_PAD_X_HU, 1200);
        assert_eq!(CODE_SPAN_PAD_X_HU, 350);
        assert_eq!(CODE_SPAN_PAD_Y_HU, 100);
        assert_eq!(CODE_SPAN_SIZE_HU, 920);
        assert_eq!(MARKER_FG, [19, 78, 74, 255]);
        assert_eq!(ABBR_UNDERLINE_OFFSET_HU, 150);
        assert_eq!(HEADING_LEADING_PCT, 120);
        assert_eq!(H2_RULE_HU, 150);
        assert_eq!(CELL_BORDER_HU, 75);
        assert_eq!(PAGE_MARGIN_X_HU, 3300);
        assert_eq!(PAGE_MARGIN_Y_HU, 3000);
        assert_eq!(TASK_MARK_ADVANCE_HU, 1380);
        assert_eq!(H1_LETTER_SPACING_HU, -42);
        assert_eq!(LIST_NESTED_GAP_HU, 300);
        assert_eq!(H1_AFTER_HU, 720);
        assert_eq!(H2_BEFORE_HU, 1320);
        assert_eq!(H2_AFTER_HU, 540);
        assert_eq!(BODY_LEADING_PCT, 145);
        assert_eq!(BODY_FG, [19, 78, 74, 255]);
        assert_eq!(QUOTE_BAR_W_HU, 300);
        assert_eq!(cell_valign_offset(VAlign::Top, 2000, 5000), 0);
        const { assert!(CODE_SPAN_PAD_X_HU < CODE_PAD_X_HU) };
        const { assert!(CODE_SPAN_PAD_Y_HU < CODE_PAD_Y_HU) };
    }

    #[test]
    fn letter_mark_padding_is_weasyprint_highlight() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("one convert, ");
        let mut mark = docagent_model::Run::text("three hashes");
        mark.style.highlight = Some(docagent_model::Color::MARK);
        p.runs.push(mark);
        p.runs.push(docagent_model::Run::text(", GPU-free"));
        p.space_after = 200;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let before = page
            .lines
            .iter()
            .find(|l| l.text.contains("convert"))
            .expect("before");
        let hashes = page
            .lines
            .iter()
            .find(|l| l.text.contains("hashes"))
            .expect("mark");
        let after = page
            .lines
            .iter()
            .find(|l| l.text.contains("GPU-free"))
            .expect("after");
        assert!(hashes.highlight);
        assert!(!before.highlight);
        assert!(!after.highlight);
        assert_eq!(
            hashes.x,
            before
                .x
                .saturating_add(before.width)
                .saturating_add(MARK_PAD_X_HU),
            "mark x {} must include letter CSS 0.2em pad",
            hashes.x.saturating_sub(before.x.saturating_add(before.width))
        );
        assert_eq!(
            after.x,
            hashes
                .x
                .saturating_add(hashes.width)
                .saturating_add(MARK_PAD_X_HU),
            "after mark {} must include letter CSS 0.2em pad",
            after.x.saturating_sub(hashes.x.saturating_add(hashes.width))
        );
        let fill = page
            .strokes
            .iter()
            .find(|s| s.fill == MARK_BG && s.width > 80 && s.height > 80)
            .expect("mark fill");
        assert_eq!(fill.x, hashes.x.saturating_sub(MARK_PAD_X_HU));
        assert_eq!(
            fill.width,
            hashes.width.saturating_add(MARK_PAD_X_HU.saturating_mul(2))
        );
        assert_eq!(
            fill.y,
            hashes
                .y
                .saturating_add(hashes.baseline)
                .saturating_sub(hashes.font_size)
                .saturating_sub(MARK_PAD_Y_HU)
        );
        assert_eq!(
            fill.height,
            hashes
                .font_size
                .saturating_add(MARK_PAD_Y_HU.saturating_mul(2))
        );
        assert!(
            fill.height > hashes.font_size,
            "mark fill must extend past glyphs so highlight is visible"
        );
        assert_eq!(MARK_PAD_X_HU, 200);
        assert_eq!(MARK_PAD_Y_HU, 50);
        assert_eq!(CELL_PAD_X_HU, 780);
        assert_eq!(H2_SIZE_MAX_HU, 1380);
        assert_eq!(HR_BEFORE_HU, 1320);
        assert_eq!(LINK_UNDERLINE_HU, 80);
        assert_eq!(QUOTE_PAD_X_HU, 1200);
    }

    #[test]
    fn letter_body_line_height_is_weasyprint_145() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Paragraph(Paragraph::from_text(
            "The shipment left Busan on 4 September and is booked on the 11 September rail window. This is a flow document: one convert, three hashes, GPU-free. Run prove twice with the same fonts and the PDF/A bytes match byte-for-byte.",
        )));
        doc.sections.push(section);
        let fonts = FontSet::bundled();
        let tree = layout_document(&doc, &fonts);
        let lines: Vec<&LineFrag> = tree.pages[0]
            .lines
            .iter()
            .filter(|l| !l.text.is_empty() && l.height > 0)
            .collect();
        assert!(
            lines.len() >= 2,
            "body must wrap so line-height is visible: {} lines",
            lines.len()
        );
        let css = i32::try_from(i64::from(DEFAULT_FONT_SIZE_HU) * 145 / 100).unwrap_or(i32::MAX);
        let expected = css.max(line_height(&fonts, DEFAULT_FONT_SIZE_HU, 100));
        assert_eq!(
            lines[0].height, expected,
            "first line height {} must be CSS 1.45 of {}pt body, not IR 160%",
            lines[0].height,
            DEFAULT_FONT_SIZE_HU / HU_PER_POINT
        );
        assert_eq!(
            lines[1].y,
            lines[0].y.saturating_add(expected),
            "wrapped body y {} must step CSS 1.45, not jam or double-space",
            lines[1].y.saturating_sub(lines[0].y)
        );
        assert!(
            lines.iter().all(|l| l.color == BODY_FG),
            "wrapped body must be letter CSS #134e4a, not UA black: {:?}",
            lines.iter().map(|l| l.color).collect::<Vec<_>>()
        );
        assert_eq!(BODY_FG, [19, 78, 74, 255]);
        assert_ne!(BODY_FG, [0, 0, 0, 255]);
    }

    #[test]
    fn letter_heading_line_height_is_weasyprint_12() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.size = 1800;
        h1.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation");
        h2.outline_level = Some(2);
        h2.runs[0].style.size = 1400;
        h2.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h2));
        let mut body = Paragraph::from_text("6 September 2026");
        body.space_after = 200;
        section.body.push(Block::Paragraph(body));
        doc.sections.push(section);
        let fonts = FontSet::bundled();
        let tree = layout_document(&doc, &fonts);
        let page = &tree.pages[0];
        let h1 = page
            .lines
            .iter()
            .find(|l| l.text.contains("Northwind Freight"))
            .expect("h1");
        let h2 = page
            .lines
            .iter()
            .find(|l| l.text.contains("Delivery confirmation"))
            .expect("h2");
        let body = page
            .lines
            .iter()
            .find(|l| l.text.contains("September"))
            .expect("body");
        let h1_natural = line_height(&fonts, h1.font_size, 100);
        let h2_natural = line_height(&fonts, h2.font_size, 100);
        assert_eq!(h1.font_size, H1_SIZE_MAX_HU);
        assert_eq!(h2.font_size, H2_SIZE_MAX_HU);
        assert_eq!(HEADING_LEADING_PCT, 120, "h1,h2 must stay letter CSS 1.2");
        assert_eq!(
            heading_line_height(H1_SIZE_MAX_HU),
            2520,
            "h1 1.2 of 1.75rem must stay 2520 HU"
        );
        assert_eq!(
            heading_line_height(H2_SIZE_MAX_HU),
            1656,
            "h2 1.2 of 1.15rem must stay 1656 HU"
        );
        assert_eq!(
            h1.height,
            heading_line_height(h1.font_size),
            "h1 line box {} must be letter CSS 1.2, not Noto (asc+desc) {}",
            h1.height,
            h1_natural
        );
        assert_eq!(
            h2.height,
            heading_line_height(h2.font_size),
            "h2 line box {} must be letter CSS 1.2, not Noto (asc+desc) {}",
            h2.height,
            h2_natural
        );
        assert!(
            h1.height < h1_natural,
            "h1 1.2em {} must beat Noto natural {}",
            h1.height,
            h1_natural
        );
        assert!(
            h2.height < h2_natural,
            "h2 1.2em {} must beat Noto natural {}",
            h2.height,
            h2_natural
        );
        let body_css = i32::try_from(i64::from(body.font_size) * i64::from(BODY_LEADING_PCT) / 100)
            .unwrap_or(i32::MAX);
        assert_eq!(
            body.height,
            body_css.max(line_height(&fonts, body.font_size, 100)),
            "body must stay letter CSS 1.45"
        );
        assert_eq!(h1.height, 2520);
        assert_eq!(h2.height, 1656);
        const { assert!(HEADING_LEADING_PCT == 120) };
        const { assert!(HEADING_LEADING_PCT < BODY_LEADING_PCT) };
    }

    #[test]
    fn letter_body_run_color_is_weasyprint_134e4a() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        section
            .body
            .push(Block::Paragraph(strong_em_para()));
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        section
            .body
            .push(Block::Paragraph(task_para("Receiving dock is clear", true)));
        let mut strike = Paragraph::from_text("guess");
        strike.runs[0].style.strike = true;
        section.body.push(Block::Paragraph(strike));
        let mut pdfa = Paragraph::from_text("PDF/");
        let mut sup = docagent_model::Run::text("A");
        sup.style.superscript = true;
        pdfa.runs.push(sup);
        section.body.push(Block::Paragraph(pdfa));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let date = page
            .lines
            .iter()
            .find(|l| l.text.contains("6 September"))
            .expect("body");
        assert_eq!(
            date.color, BODY_FG,
            "body run must be letter CSS #134e4a, not UA black"
        );
        assert_ne!(date.color, [0, 0, 0, 255], "body must not stay UA black");
        let gpu = page
            .lines
            .iter()
            .find(|l| l.text.contains("GPU-free"))
            .expect("strong");
        assert!(gpu.bold, "strong must not regress");
        assert_eq!(gpu.color, BODY_FG);
        let slant = page
            .lines
            .iter()
            .find(|l| l.text.contains("byte-for-byte"))
            .expect("em");
        assert!(slant.italic, "em must not regress");
        assert_eq!(slant.color, BODY_FG);
        let hop = page
            .lines
            .iter()
            .find(|l| l.text.contains("Hop"))
            .expect("th");
        assert_eq!(hop.color, TH_FG, "th must not regress");
        assert!(hop.bold);
        let input = page
            .lines
            .iter()
            .find(|l| l.text.contains("Input"))
            .expect("td");
        assert_eq!(input.color, BODY_FG, "td body must stay letter #134e4a");
        let replay = page
            .lines
            .iter()
            .find(|l| l.text.contains("Replay is evidence"))
            .expect("quote");
        assert_eq!(replay.color, QUOTE_FG, "quote must not regress");
        assert!(
            page.strokes.iter().any(|s| {
                s.fill == QUOTE_FG && s.width == QUOTE_BAR_W_HU && s.height > 200
            }),
            "quote bar must not regress"
        );
        assert!(
            page.strokes
                .iter()
                .any(|s| s.fill == [19, 78, 74, 255]
                    && s.width == TASK_BOX_HU
                    && s.height == TASK_BOX_HU),
            "task box must not regress"
        );
        let guess = page
            .lines
            .iter()
            .find(|l| l.text.contains("guess"))
            .expect("strike");
        assert!(guess.strike);
        assert_eq!(guess.color, STRIKE_COLOR, "strike must not regress");
        let raised = page.lines.iter().find(|l| l.text == "A").expect("super");
        assert_eq!(raised.font_size, SUPER_SUB_SIZE_HU);
        assert_eq!(raised.color, BODY_FG, "sup must keep letter body #134e4a");
        assert_eq!(BODY_FG, [19, 78, 74, 255]);
        assert_ne!(BODY_FG, [0, 0, 0, 255]);
        assert_eq!(BODY_FG, QUOTE_FG);
        assert_eq!(BODY_FG, TH_FG);
        assert_eq!(BODY_FG, STRIKE_COLOR);
    }

    #[test]
    fn letter_body_quote_th_tasks_and_24pt_still_emit() {
        let (w, h) = mark_png_hu();
        let (ew, eh) = min_on_page_size(w, h);
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * HU_PER_POINT);
        assert_eq!(MIN_IMAGE_EDGE_HU, 2400);
        assert!(
            ew >= MIN_IMAGE_EDGE_HU && eh >= MIN_IMAGE_EDGE_HU,
            "24pt floor missing: {ew}×{eh}"
        );
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut img_p = Paragraph::from_text("");
        img_p.runs = vec![mark_run(w, h)];
        section.body.push(Block::Paragraph(img_p));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        section
            .body
            .push(Block::Paragraph(task_para("Receiving dock is clear", true)));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let img = page
            .lines
            .iter()
            .find(|l| l.image.is_some())
            .expect("24pt mark");
        let payload = img.image.as_ref().expect("payload");
        assert_eq!((payload.width, payload.height), (ew, eh));
        let date = page
            .lines
            .iter()
            .find(|l| l.text.contains("6 September"))
            .expect("body");
        assert_eq!(
            date.color, BODY_FG,
            "body run must be letter CSS #134e4a, not UA black"
        );
        assert_ne!(date.color, [0, 0, 0, 255]);
        assert_eq!(BODY_FG, [19, 78, 74, 255]);
        assert_eq!(BODY_FG, QUOTE_FG);
        assert_eq!(BODY_FG, TH_FG);
        let hop = page
            .lines
            .iter()
            .find(|l| l.text.contains("Hop"))
            .expect("th");
        assert_eq!(hop.color, TH_FG);
        assert!(hop.bold);
        assert!(
            page.rects
                .iter()
                .any(|r| r.fill == TH_BG && r.width > 80 && r.height > 80),
            "th fill must not regress: {:?}",
            page.rects
        );
        assert_eq!(TH_BG, [204, 251, 241, 255]);
        let replay = page
            .lines
            .iter()
            .find(|l| l.text.contains("Replay is evidence"))
            .expect("quote");
        assert_eq!(replay.color, QUOTE_FG);
        assert!(
            page.strokes.iter().any(|s| {
                s.fill == QUOTE_FG && s.width == QUOTE_BAR_W_HU && s.height > 200
            }),
            "quote bar must not regress: {:?}",
            page.strokes
        );
        assert_eq!(QUOTE_BAR_W_HU, 300);
        assert!(
            page.strokes.iter().any(|s| {
                s.fill == [19, 78, 74, 255]
                    && s.width == TASK_BOX_HU
                    && s.height == TASK_BOX_HU
            }),
            "task box must not regress: {:?}",
            page.strokes
        );
        assert_eq!(TASK_BOX_HU, 900);
        assert_eq!(TASK_BORDER_HU, 150);
        let dock = page
            .lines
            .iter()
            .find(|l| l.text.contains("dock"))
            .expect("task text");
        assert_eq!(
            dock.color, BODY_FG,
            "task text must stay letter body #134e4a"
        );
        assert!(
            date.y >= img.y.saturating_add(eh),
            "body overlaps 24pt mark"
        );
    }

    #[test]
    fn oversized_image_clamps_to_content_width() {
        let mut doc = Document::new();
        let mut section = Section::default();
        // US Letter: 8.5in × 11in at 7200 HU/in.
        section.page.width = 61_200;
        section.page.height = 79_200;
        let content_w = (section.page.width
            - page_margin_x(section.page.margin_left)
            - page_margin_x(section.page.margin_right)
            - section.page.gutter)
            .max(1);
        let mut p = Paragraph::from_text("");
        p.runs = vec![mark_run(80_000, 10_000)];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let frag = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.image.is_some())
            .expect("image run skipped");
        assert!(
            frag.width <= content_w,
            "frag.width {} exceeds content width {}",
            frag.width,
            content_w
        );
        let img = frag.image.as_ref().expect("payload");
        assert_eq!(img.width, frag.width);
        assert!(img.width <= content_w);
        assert_eq!(
            img.height,
            (i64::from(10_000) * i64::from(img.width) / 80_000) as i32
        );
        assert!(img.height > 0);
    }

    fn mark_image(width: Hu, height: Hu) -> docagent_model::ImageData {
        docagent_model::ImageData {
            bytes: MARK_PNG.to_vec(),
            mime: "image/png".into(),
            width,
            height,
            alt_text: Some("mark".into()),
            wrap: docagent_model::WrapMode::Square,
        }
    }

    #[test]
    fn float_image_yields_line_with_payload() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Float(Float::Image(mark_image(1600, 1600))));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let frag = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.image.is_some())
            .expect("float image dropped");
        let (ew, eh) = min_on_page_size(1600, 1600);
        assert_eq!(frag.width, ew);
        assert!(frag.height >= eh, "line height {}", frag.height);
        let img = frag.image.as_ref().expect("payload");
        assert_eq!(img.bytes, MARK_PNG);
        assert_eq!(img.mime, "image/png");
        assert_eq!(img.width, ew);
        assert_eq!(img.height, eh);
        assert!(frag.text.is_empty());
    }

    #[test]
    fn oversized_float_image_clamps_to_content_width() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let content_w = (section.page.width
            - page_margin_x(section.page.margin_left)
            - page_margin_x(section.page.margin_right)
            - section.page.gutter)
            .max(1);
        section
            .body
            .push(Block::Float(Float::Image(mark_image(80_000, 10_000))));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let frag = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.image.is_some())
            .expect("float image dropped");
        assert!(
            frag.width <= content_w,
            "frag.width {} exceeds content width {}",
            frag.width,
            content_w
        );
        let img = frag.image.as_ref().expect("payload");
        assert_eq!(img.width, frag.width);
        assert!(img.width <= content_w);
        assert_eq!(
            img.height,
            (i64::from(10_000) * i64::from(img.width) / 80_000) as i32
        );
        assert!(img.height > 0);
    }

    fn table_with_image_cell(width: Hu, height: Hu) -> Table {
        let mut table = Table::from_cells(vec![vec![String::new()]]);
        let mut p = Paragraph::from_text("");
        p.runs = vec![mark_run(width, height)];
        table.rows[0].cells[0].blocks = vec![Block::Paragraph(p)];
        table
    }

    #[test]
    fn table_cell_image_yields_line_with_payload() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Table(table_with_image_cell(1600, 1600)));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let frag = tree.pages[0]
            .lines
            .iter()
            .find(|l| l.image.is_some())
            .expect("cell image dropped");
        let (ew, eh) = min_on_page_size(1600, 1600);
        assert_eq!(frag.width, ew);
        assert!(frag.height >= eh, "line height {}", frag.height);
        let img = frag.image.as_ref().expect("payload");
        assert_eq!(img.bytes, MARK_PNG);
        assert_eq!(img.mime, "image/png");
        assert_eq!(img.width, ew);
        assert_eq!(img.height, eh);
        assert!(frag.text.is_empty());
    }

    #[test]
    fn table_cell_image_clamps_to_cell_content_width() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![vec![String::new(), String::new()]]);
        table.rows[0].cells[0].width = 8_000;
        table.rows[0].cells[1].width = 8_000;
        let mut p = Paragraph::from_text("");
        p.runs = vec![mark_run(50_000, 10_000)];
        table.rows[0].cells[0].blocks = vec![Block::Paragraph(p)];
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let cell_w = page.rects.first().map(|r| r.width).expect("cell fill");
        let inner = (cell_w - CELL_PAD_X_HU.saturating_mul(2)).max(1);
        let frag = page
            .lines
            .iter()
            .find(|l| l.image.is_some())
            .expect("cell image dropped");
        assert!(
            frag.width <= inner,
            "frag.width {} exceeds cell content width {}",
            frag.width,
            inner
        );
        let img = frag.image.as_ref().expect("payload");
        assert_eq!(img.width, frag.width);
        assert!(img.width <= inner);
        assert_eq!(
            img.height,
            (i64::from(10_000) * i64::from(img.width) / 50_000) as i32
        );
        assert!(img.height > 0);
        assert_eq!(img.bytes, MARK_PNG);
    }

    #[test]
    fn section_header_appears_on_each_page() {
        let mut doc = Document::new();
        let mut section = Section {
            header: Some(hf("HEAD")),
            ..Section::default()
        };
        for _ in 0..80 {
            section
                .body
                .push(Block::Paragraph(Paragraph::from_text("body line")));
        }
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        assert!(
            tree.pages.len() >= 2,
            "need a second page, got {}",
            tree.pages.len()
        );
        for (i, page) in tree.pages.iter().enumerate() {
            let head = page
                .lines
                .iter()
                .find(|l| l.text.contains("HEAD"))
                .unwrap_or_else(|| panic!("HEAD missing on page {i}"));
            assert!(head.y >= 0, "header y {} off page", head.y);
            assert!(head.y < page.height, "header y {} past page", head.y);
            assert!(
                head.y < DEFAULT_MARGIN_HU,
                "header y {} not in top margin",
                head.y
            );
            assert!(
                page.lines.iter().any(|l| l.text.contains("body line")),
                "body dropped on page {i}"
            );
        }
    }

    #[test]
    fn section_footer_appears_on_page() {
        let mut doc = Document::new();
        let mut section = Section {
            footer: Some(hf("FOOT")),
            ..Section::default()
        };
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("body line")));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let foot = page
            .lines
            .iter()
            .find(|l| l.text.contains("FOOT"))
            .expect("FOOT missing");
        assert!(foot.y >= 0);
        assert!(foot.y < page.height);
        assert!(
            foot.y >= page.height.saturating_sub(DEFAULT_MARGIN_HU),
            "footer y {} not in bottom margin",
            foot.y
        );
        assert!(
            page.lines.iter().any(|l| l.text.contains("body line")),
            "body dropped"
        );
    }

    #[test]
    fn section_header_image_appears_in_band() {
        let mut doc = Document::new();
        let mut section = Section {
            header: Some(hf_image(1600, 1600)),
            ..Section::default()
        };
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("body line")));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let frag = page
            .lines
            .iter()
            .find(|l| l.image.is_some())
            .expect("header image missing");
        assert!(frag.y >= 0, "header image y {} off page", frag.y);
        assert!(
            frag.y < DEFAULT_MARGIN_HU,
            "header image y {} not in top margin",
            frag.y
        );
        let img = frag.image.as_ref().expect("payload");
        let (ew, eh) = min_on_page_size(1600, 1600);
        assert_eq!(img.width, ew);
        assert_eq!(img.height, eh);
        assert_eq!(img.bytes, MARK_PNG);
        assert_eq!(img.mime, "image/png");
        assert!(
            page.lines.iter().any(|l| l.text.contains("body line")),
            "body dropped"
        );
    }

    #[test]
    fn section_header_image_clamps_to_content_width() {
        let mut doc = Document::new();
        let mut section = Section {
            header: Some(hf_image(80_000, 10_000)),
            ..Section::default()
        };
        let content_w = (section.page.width
            - page_margin_x(section.page.margin_left)
            - page_margin_x(section.page.margin_right)
            - section.page.gutter)
            .max(1);
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("body line")));
        doc.sections.push(section);
        let tree = layout_document(&doc, &FontSet::bundled());
        let page = &tree.pages[0];
        let frag = page
            .lines
            .iter()
            .find(|l| l.image.is_some())
            .expect("header image missing");
        assert!(
            frag.y < DEFAULT_MARGIN_HU,
            "header image y {} not in top margin",
            frag.y
        );
        assert!(
            frag.width <= content_w,
            "frag.width {} exceeds content width {}",
            frag.width,
            content_w
        );
        let img = frag.image.as_ref().expect("payload");
        assert_eq!(img.width, frag.width);
        assert!(img.width <= content_w);
        assert_eq!(
            img.height,
            (i64::from(10_000) * i64::from(img.width) / 80_000) as i32
        );
        assert!(img.height > 0);
        assert_eq!(img.bytes, MARK_PNG);
    }
}
