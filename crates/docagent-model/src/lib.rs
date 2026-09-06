//! Format-neutral flow-document IR.
//!
//! External dependencies: `serde` derive only. No format identifier lives here.
//! Lengths are HWPUNIT (`i32`, 1/7200 inch). Twips convert by multiplying by 5.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

/// HWPUNIT: 1/7200 of an inch. Canonical IR length.
pub type Hu = i32;
/// Accumulator for HWPUNIT products and sums.
pub type Hu64 = i64;
/// Angle in 1/1000 of a degree.
pub type MilliDegrees = i32;

/// 1 inch in HWPUNIT. HWP 5.0 spec §2 unit definition; 7200 = 1440 twips × 5.
pub const HU_PER_INCH: i32 = 7200;
/// 1 twip (1/1440 inch) in HWPUNIT. ECMA-376 uses twips; 7200/1440 = 5.
pub const HU_PER_TWIP: i32 = 5;
/// 1 PostScript point (1/72 inch) in HWPUNIT. 7200/72 = 100.
pub const HU_PER_POINT: i32 = 100;

/// A4 width: 210 mm × 7200 / 25.4, rounded to nearest HWPUNIT.
pub const A4_WIDTH_HU: Hu = 59528;
/// A4 height: 297 mm × 7200 / 25.4, rounded to nearest HWPUNIT.
pub const A4_HEIGHT_HU: Hu = 84189;
/// 30 mm default margin: 30 × 7200 / 25.4.
pub const DEFAULT_MARGIN_HU: Hu = 8504;
/// Default body text size 10 pt.
pub const DEFAULT_FONT_SIZE_HU: Hu = 1000;

pub fn twips_to_hu(twips: i32) -> Hu {
    twips.saturating_mul(HU_PER_TWIP)
}

pub fn hu_to_twips(hu: Hu) -> i32 {
    hu / HU_PER_TWIP
}

pub fn points_to_hu(points: i32) -> Hu {
    points.saturating_mul(HU_PER_POINT)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const WHITE: Color = Color {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    pub const TRANSPARENT: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };

    pub fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub sections: Vec<Section>,
    pub styles: StyleSheet,
    pub fonts: Vec<FontFace>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Document {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        for section in &self.sections {
            collect_blocks_text(&section.body, &mut out);
            for note in &section.footnotes {
                collect_blocks_text(&note.blocks, &mut out);
            }
            for note in &section.endnotes {
                collect_blocks_text(&note.blocks, &mut out);
            }
        }
        out
    }

    pub fn paragraph_count(&self) -> usize {
        self.sections
            .iter()
            .map(|s| count_paragraphs(&s.body))
            .sum()
    }

    pub fn table_count(&self) -> usize {
        self.sections.iter().map(|s| count_tables(&s.body)).sum()
    }
}

fn collect_blocks_text(blocks: &[Block], out: &mut String) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => {
                if !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str(&p.plain_text());
            }
            Block::Table(t) => {
                for row in &t.rows {
                    for cell in &row.cells {
                        collect_blocks_text(&cell.blocks, out);
                    }
                }
            }
            Block::Float(f) => match f {
                Float::Shape(s) => {
                    if let Some(text) = &s.alt_text {
                        if !out.is_empty() && !out.ends_with('\n') {
                            out.push('\n');
                        }
                        out.push_str(text);
                    }
                }
                Float::Image(img) => {
                    if let Some(text) = &img.alt_text {
                        if !out.is_empty() && !out.ends_with('\n') {
                            out.push('\n');
                        }
                        out.push_str(text);
                    }
                }
                Float::Equation(eq) => {
                    if !out.is_empty() && !out.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push_str(&eq.tex);
                }
            },
            Block::Break(_) => {}
        }
    }
}

fn count_paragraphs(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .map(|b| match b {
            Block::Paragraph(_) => 1,
            Block::Table(t) => t
                .rows
                .iter()
                .flat_map(|r| r.cells.iter())
                .map(|c| count_paragraphs(&c.blocks))
                .sum(),
            _ => 0,
        })
        .sum()
}

fn count_tables(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .map(|b| match b {
            Block::Table(t) => {
                1 + t
                    .rows
                    .iter()
                    .flat_map(|r| r.cells.iter())
                    .map(|c| count_tables(&c.blocks))
                    .sum::<usize>()
            }
            _ => 0,
        })
        .sum()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub page: PageSetup,
    pub columns: ColumnSetup,
    pub body: Vec<Block>,
    pub header: Option<HeaderFooter>,
    pub even_header: Option<HeaderFooter>,
    pub first_header: Option<HeaderFooter>,
    pub footer: Option<HeaderFooter>,
    pub even_footer: Option<HeaderFooter>,
    pub first_footer: Option<HeaderFooter>,
    pub background: Option<Background>,
    pub footnotes: Vec<Note>,
    pub endnotes: Vec<Note>,
    pub page_number: PageNumbering,
    pub start_page_number: Option<u32>,
}

impl Default for Section {
    fn default() -> Self {
        Self {
            page: PageSetup::a4(),
            columns: ColumnSetup::single(),
            body: Vec::new(),
            header: None,
            even_header: None,
            first_header: None,
            footer: None,
            even_footer: None,
            first_footer: None,
            background: None,
            footnotes: Vec::new(),
            endnotes: Vec::new(),
            page_number: PageNumbering::default(),
            start_page_number: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageSetup {
    pub width: Hu,
    pub height: Hu,
    pub margin_left: Hu,
    pub margin_right: Hu,
    pub margin_top: Hu,
    pub margin_bottom: Hu,
    pub header_distance: Hu,
    pub footer_distance: Hu,
    pub gutter: Hu,
    pub landscape: bool,
    pub gutter_at_top: Option<bool>,
    pub mirror_margins: Option<bool>,
    pub paper_code: Option<u8>,
}

impl PageSetup {
    pub fn a4() -> Self {
        Self {
            width: A4_WIDTH_HU,
            height: A4_HEIGHT_HU,
            margin_left: DEFAULT_MARGIN_HU,
            margin_right: DEFAULT_MARGIN_HU,
            margin_top: DEFAULT_MARGIN_HU,
            margin_bottom: DEFAULT_MARGIN_HU,
            header_distance: DEFAULT_MARGIN_HU / 2,
            footer_distance: DEFAULT_MARGIN_HU / 2,
            gutter: 0,
            landscape: false,
            gutter_at_top: None,
            mirror_margins: None,
            paper_code: None,
        }
    }

    pub fn content_width(&self) -> Hu {
        (self.width - self.margin_left - self.margin_right - self.gutter).max(1)
    }

    pub fn content_height(&self) -> Hu {
        (self.height - self.margin_top - self.margin_bottom).max(1)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColumnSetup {
    pub count: u8,
    pub gap: Hu,
    pub equal_width: bool,
    pub widths: Vec<Hu>,
}

impl ColumnSetup {
    pub fn single() -> Self {
        Self {
            count: 1,
            gap: 0,
            equal_width: true,
            widths: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HeaderFooter {
    pub blocks: Vec<Block>,
    pub different_first: Option<bool>,
    pub different_odd_even: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Background {
    pub fill: Option<Color>,
    pub image: Option<ImageData>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub id: u32,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PageNumbering {
    pub format: NumberFormat,
    pub position: Option<PageNumberPosition>,
    pub start_at: Option<u32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NumberFormat {
    #[default]
    Decimal,
    RomanLower,
    RomanUpper,
    AlphaLower,
    AlphaUpper,
    Circled,
    Bullet,
    Task,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageNumberPosition {
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
    Inside,
    Outside,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Block {
    Paragraph(Paragraph),
    Table(Table),
    Float(Float),
    Break(BreakKind),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BreakKind {
    Page,
    Column,
    Section,
    Thematic,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Paragraph {
    pub style: Option<String>,
    pub runs: Vec<Run>,
    pub numbering: Option<NumberingRef>,
    pub alignment: Alignment,
    pub indent_left: Hu,
    pub indent_right: Hu,
    pub indent_first: Hu,
    pub space_before: Hu,
    pub space_after: Hu,
    pub line_spacing: LineSpacing,
    pub keep_with_next: Option<bool>,
    pub keep_together: Option<bool>,
    pub widow_control: Option<bool>,
    pub outline_level: Option<u8>,
    pub quote: bool,
    pub code_block: bool,
    pub layout_hints: Vec<LayoutHint>,
}

impl Paragraph {
    pub fn from_text(text: impl Into<String>) -> Self {
        Self {
            style: None,
            runs: vec![Run::text(text)],
            numbering: None,
            alignment: Alignment::Start,
            indent_left: 0,
            indent_right: 0,
            indent_first: 0,
            space_before: 0,
            space_after: 0,
            line_spacing: LineSpacing::Percent(160),
            keep_with_next: None,
            keep_together: None,
            widow_control: None,
            outline_level: None,
            quote: false,
            code_block: false,
            layout_hints: Vec::new(),
        }
    }

    pub fn plain_text(&self) -> String {
        let mut s = String::new();
        for run in &self.runs {
            s.push_str(run.display_text());
        }
        s
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Alignment {
    #[default]
    Start,
    End,
    Center,
    Justify,
    Distribute,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineSpacing {
    /// Percent of font size. 160 means 160%. Hangul default body spacing.
    Percent(u16),
    /// Absolute leading in HWPUNIT.
    Absolute(Hu),
    /// At-least leading in HWPUNIT (OOXML `w:lineRule="atLeast"`).
    AtLeast(Hu),
}

impl Default for LineSpacing {
    fn default() -> Self {
        Self::Percent(160)
    }
}

/// Hangul-stored line geometry. Layout must not read this.
/// The conformance harness uses it as oracle 1 (HWP LineSeg).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutHint {
    pub text_start: u32,
    pub x: Hu,
    pub y: Hu,
    pub width: Hu,
    pub line_height: Hu,
    pub text_height: Hu,
    pub baseline: Hu,
    pub spacing: Hu,
    pub flags: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub style: CharStyle,
    pub content: RunContent,
}

impl Run {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            style: CharStyle::default(),
            content: RunContent::Text(text.into()),
        }
    }

    pub fn display_text(&self) -> &str {
        match &self.content {
            RunContent::Text(t) => t,
            RunContent::Inline(InlineObject::Hyperlink { display, .. }) => display,
            RunContent::Inline(_) => "",
        }
    }

    pub fn hyperlink(display: impl Into<String>, target: impl Into<String>) -> Self {
        let mut run = Self {
            style: CharStyle::default(),
            content: RunContent::Inline(InlineObject::Hyperlink {
                target: target.into(),
                display: display.into(),
            }),
        };
        run.style.underline = Underline::Single;
        run
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RunContent {
    Text(String),
    Inline(InlineObject),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum InlineObject {
    Image(ImageData),
    Tab,
    LineBreak,
    FootnoteRef {
        id: u32,
    },
    EndnoteRef {
        id: u32,
    },
    Field {
        kind: FieldKind,
        code: String,
        display: String,
    },
    Bookmark {
        name: String,
    },
    Hyperlink {
        target: String,
        display: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldKind {
    Date,
    PageNumber,
    NumPages,
    Hyperlink,
    MailMerge,
    CrossRef,
    Formula,
    ClickHere,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CharStyle {
    pub font: Option<String>,
    pub latin_font: Option<String>,
    pub size: Hu,
    pub bold: bool,
    pub italic: bool,
    pub underline: Underline,
    pub strike: bool,
    pub code: bool,
    pub color: Color,
    pub highlight: Option<Color>,
    pub superscript: bool,
    pub subscript: bool,
    pub scale_percent: Option<u16>,
    pub spacing: Hu,
    pub language: Option<String>,
}

impl Default for CharStyle {
    fn default() -> Self {
        Self {
            font: None,
            latin_font: None,
            size: DEFAULT_FONT_SIZE_HU,
            bold: false,
            italic: false,
            underline: Underline::None,
            strike: false,
            code: false,
            color: Color::BLACK,
            highlight: None,
            superscript: false,
            subscript: false,
            scale_percent: None,
            spacing: 0,
            language: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Underline {
    #[default]
    None,
    Single,
    Double,
    Dotted,
    Wave,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NumberingRef {
    pub definition_id: u32,
    pub level: u8,
    pub start: Option<u32>,
    pub format: Option<NumberFormat>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Table {
    pub rows: Vec<TableRow>,
    pub borders: TableBorders,
    pub split_policy: SplitPolicy,
    pub width: Option<Hu>,
    pub alignment: Alignment,
    pub cell_spacing: Hu,
    pub indent: Hu,
    pub header_row_count: u8,
    pub layout_hints: Vec<LayoutHint>,
}

impl Table {
    pub fn from_cells(cells: Vec<Vec<String>>) -> Self {
        let rows = cells
            .into_iter()
            .map(|row| TableRow {
                cells: row
                    .into_iter()
                    .map(|text| TableCell {
                        blocks: vec![Block::Paragraph(Paragraph::from_text(text))],
                        ..TableCell::default()
                    })
                    .collect(),
                height: None,
                header: false,
                cant_split: None,
            })
            .collect();
        Self {
            rows,
            borders: TableBorders::default(),
            split_policy: SplitPolicy::Allow,
            width: None,
            alignment: Alignment::Start,
            cell_spacing: 0,
            indent: 0,
            header_row_count: 0,
            layout_hints: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TableRow {
    pub cells: Vec<TableCell>,
    pub height: Option<Hu>,
    pub header: bool,
    pub cant_split: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TableCell {
    pub blocks: Vec<Block>,
    pub width: Hu,
    pub merge: CellMerge,
    pub borders: Option<TableBorders>,
    pub shading: Option<Color>,
    pub valign: VAlign,
    pub padding: Padding,
}

impl Default for TableCell {
    fn default() -> Self {
        Self {
            blocks: Vec::new(),
            width: 10000,
            merge: CellMerge::None,
            borders: None,
            shading: None,
            valign: VAlign::Top,
            padding: Padding::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CellMerge {
    #[default]
    None,
    HorizontalHead { span: u16 },
    HorizontalRest,
    VerticalHead { span: u16 },
    VerticalRest,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VAlign {
    #[default]
    Top,
    Middle,
    Bottom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Padding {
    pub left: Hu,
    pub right: Hu,
    pub top: Hu,
    pub bottom: Hu,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitPolicy {
    #[default]
    Allow,
    KeepTogether,
    RepeatHeader,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TableBorders {
    pub top: Border,
    pub bottom: Border,
    pub left: Border,
    pub right: Border,
    pub inside_h: Border,
    pub inside_v: Border,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Border {
    pub style: BorderStyle,
    pub width: Hu,
    pub color: Color,
}

impl Default for Border {
    fn default() -> Self {
        Self {
            style: BorderStyle::Single,
            width: 10,
            color: Color::BLACK,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BorderStyle {
    None,
    #[default]
    Single,
    Double,
    Dotted,
    Dashed,
    Thick,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Float {
    Shape(Shape),
    Image(ImageData),
    Equation(Equation),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    pub kind: ShapeKind,
    pub x: Hu,
    pub y: Hu,
    pub width: Hu,
    pub height: Hu,
    pub rotation: MilliDegrees,
    pub fill: Option<Color>,
    pub stroke: Option<Border>,
    pub wrap: WrapMode,
    pub alt_text: Option<String>,
    pub behind_text: Option<bool>,
    pub anchor: Anchor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShapeKind {
    Rectangle,
    Ellipse,
    Line,
    Polygon,
    Arc,
    Curve,
    Container,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WrapMode {
    Inline,
    Square,
    Tight,
    Through,
    TopAndBottom,
    Behind,
    InFront,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Anchor {
    Paragraph,
    Page,
    Margin,
    Character,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageData {
    pub bytes: Vec<u8>,
    pub mime: String,
    pub width: Hu,
    pub height: Hu,
    pub alt_text: Option<String>,
    pub wrap: WrapMode,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Equation {
    pub tex: String,
    pub width: Option<Hu>,
    pub height: Option<Hu>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StyleSheet {
    pub paragraph: Vec<NamedStyle>,
    pub character: Vec<NamedStyle>,
    pub numbering: Vec<NumberingDef>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NamedStyle {
    pub id: String,
    pub name: String,
    pub based_on: Option<String>,
    pub para: Option<ParagraphStyleBits>,
    pub char: Option<CharStyle>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParagraphStyleBits {
    pub alignment: Option<Alignment>,
    pub indent_left: Option<Hu>,
    pub space_before: Option<Hu>,
    pub space_after: Option<Hu>,
    pub line_spacing: Option<LineSpacing>,
    pub outline_level: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NumberingDef {
    pub id: u32,
    pub levels: Vec<NumberingLevel>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NumberingLevel {
    pub level: u8,
    pub format: NumberFormat,
    pub prefix: String,
    pub suffix: String,
    pub indent: Hu,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FontFace {
    pub name: String,
    pub family: FontFamily,
    pub embedded: Option<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FontFamily {
    Hangul,
    Latin,
    Hanja,
    Japanese,
    Symbol,
    User,
    Other,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: DiagnosticCode,
    pub message: String,
    pub location: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticCode {
    ParseLoss,
    FontSubstitute,
    Unsupported,
    RoundtripLoss,
    LayoutHintIgnored,
}

impl Diagnostic {
    pub fn parse_loss(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            code: DiagnosticCode::ParseLoss,
            message: message.into(),
            location: None,
        }
    }

    pub fn roundtrip_loss(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            code: DiagnosticCode::RoundtripLoss,
            message: message.into(),
            location: None,
        }
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Info,
            code: DiagnosticCode::Unsupported,
            message: message.into(),
            location: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twip_conversion_is_exact_multiple() {
        assert_eq!(twips_to_hu(1440), HU_PER_INCH);
        assert_eq!(hu_to_twips(7200), 1440);
        assert_eq!(points_to_hu(1), 100);
    }

    #[test]
    fn a4_matches_iso216_in_hwpunit() {
        assert_eq!(A4_WIDTH_HU, 59528);
        assert_eq!(A4_HEIGHT_HU, 84189);
    }

    #[test]
    fn plain_text_walks_paragraphs_and_tables() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("alpha")));
        section.body.push(Block::Table(Table::from_cells(vec![
            vec!["b".into(), "c".into()],
        ])));
        doc.sections.push(section);
        let text = doc.plain_text();
        assert!(text.contains("alpha"));
        assert!(text.contains('b'));
        assert!(text.contains('c'));
        assert_eq!(doc.paragraph_count(), 3);
        assert_eq!(doc.table_count(), 1);
    }
}
