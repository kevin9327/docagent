//! Markdown ↔ IR. Headings, paragraphs, and pipe tables.
//!
//! This is a document interchange codec, not a full CommonMark parser.

#![forbid(unsafe_code)]

use docagent_model::{
    Block, BreakKind, Color, Document, ImageData, InlineObject, LineSpacing, NumberFormat,
    NumberingRef, Padding, Paragraph, Run, RunContent, Section, Table, TableCell, WrapMode,
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
    if bytes.starts_with(b"HWP Document File")
        || bytes.starts_with(b"<?xml")
        || bytes.starts_with(b"{")
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
    let mut in_fence = false;
    let mut fence_lines: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_end();
        if in_fence {
            if trimmed.trim_start().starts_with("```") {
                flush_fence(&mut section, &mut fence_lines);
                in_fence = false;
            } else {
                fence_lines.push(trimmed.to_string());
            }
            continue;
        }
        if trimmed.trim_start().starts_with("```") {
            flush_table(&mut section, &mut table_rows);
            in_fence = true;
            fence_lines.clear();
            continue;
        }
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
        if is_thematic_break(content) {
            section.body.push(Block::Break(BreakKind::Thematic));
        } else if let Some(rest) = content.strip_prefix("# ") {
            section
                .body
                .push(Block::Paragraph(heading(rest, 1, H1_SIZE_HU, 200, 400)));
        } else if let Some(rest) = content.strip_prefix("## ") {
            section
                .body
                .push(Block::Paragraph(heading(rest, 2, H2_SIZE_HU, 360, 240)));
        } else if let Some(rest) = content
            .strip_prefix("- ")
            .or_else(|| content.strip_prefix("* "))
        {
            if let Some((checked, body)) = task_item(rest) {
                section
                    .body
                    .push(Block::Paragraph(task_para(body, nest, checked)));
            } else {
                section.body.push(Block::Paragraph(bullet_item(rest, nest)));
            }
        } else if let Some((n, rest)) = ordered_item(content) {
            section
                .body
                .push(Block::Paragraph(decimal_item(rest, n, nest)));
        } else if let Some(rest) = content.strip_prefix("> ") {
            let mut p = paragraph_from_inlines(rest);
            apply_letter_quote(&mut p);
            section.body.push(Block::Paragraph(p));
        } else {
            let mut p = paragraph_from_inlines(trimmed);
            p.space_after = if paragraph_is_image_only(&p) {
                IMAGE_AFTER_HU
            } else {
                200
            };
            section.body.push(Block::Paragraph(p));
        }
    }
    flush_table(&mut section, &mut table_rows);
    if in_fence {
        flush_fence(&mut section, &mut fence_lines);
    }
    apply_letter_list_spacing(&mut section.body);
    doc.sections.push(section);
    Ok(doc)
}

fn flush_fence(section: &mut Section, lines: &mut Vec<String>) {
    let body = lines.join(" ");
    lines.clear();
    let mut p = Paragraph::from_text(body);
    apply_letter_code_block(&mut p);
    p.line_spacing = LineSpacing::Percent(BODY_LEADING_PCT);
    section.body.push(Block::Paragraph(p));
}

fn ordered_item(line: &str) -> Option<(u32, &str)> {
    let (num, rest) = line.split_once(". ")?;
    if num.is_empty() || !num.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u32 = num.parse().ok()?;
    (n > 0).then_some((n, rest))
}

fn is_thematic_break(line: &str) -> bool {
    let t = line.trim();
    if t.len() < 3 {
        return false;
    }
    let first = t.as_bytes()[0];
    if first != b'-' && first != b'*' && first != b'_' {
        return false;
    }
    t.bytes().all(|b| b == first || b == b' ') && t.bytes().filter(|b| *b == first).count() >= 3
}

fn list_nest(line: &str) -> u8 {
    let spaces = line.bytes().take_while(|b| *b == b' ').count();
    (spaces / 2).min(3) as u8
}

fn list_indent(nest: u8) -> i32 {
    LIST_INDENT_HU.saturating_mul(i32::from(nest) + 1)
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

fn paragraph_from_inlines(text: &str) -> Paragraph {
    let mut p = Paragraph::from_text("");
    p.runs = parse_runs(text);
    if p.runs.is_empty() {
        p.runs.push(Run::text(""));
    }
    p.line_spacing = LineSpacing::Percent(BODY_LEADING_PCT);
    apply_letter_body_color(&mut p);
    p
}

#[derive(Clone, Copy, Default)]
struct InlineMarks {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
    mark: bool,
    under: bool,
    super_: bool,
    sub_: bool,
}

fn parse_runs(text: &str) -> Vec<Run> {
    let chars: Vec<char> = text.chars().collect();
    let mut runs = Vec::new();
    let mut buf = String::new();
    let mut marks = InlineMarks::default();
    let mut i = 0;
    let flush = |runs: &mut Vec<Run>, buf: &mut String, marks: InlineMarks| {
        if buf.is_empty() {
            return;
        }
        let mut run = Run::text(std::mem::take(buf));
        run.style.bold = marks.bold;
        run.style.italic = marks.italic;
        run.style.strike = marks.strike;
        run.style.code = marks.code;
        if marks.code {
            apply_letter_code_font(&mut run);
        }
        if marks.mark {
            run.style.highlight = Some(docagent_model::Color::MARK);
        }
        if marks.under {
            run.style.underline = docagent_model::Underline::Single;
        }
        run.style.superscript = marks.super_;
        run.style.subscript = marks.sub_;
        runs.push(run);
    };
    while i < chars.len() {
        if chars[i] == '`' {
            flush(&mut runs, &mut buf, marks);
            marks.code = !marks.code;
            i += 1;
            continue;
        }
        if !marks.code
            && chars[i] == '!'
            && let Some((alt, src, next)) = parse_md_image(&chars, i)
        {
            flush(&mut runs, &mut buf, marks);
            runs.push(image_run(alt, &src));
            i = next;
            continue;
        }
        if !marks.code
            && chars[i] == '['
            && let Some((display, target, next)) = parse_md_link(&chars, i)
        {
            flush(&mut runs, &mut buf, marks);
            runs.push(Run::hyperlink(display, target));
            i = next;
            continue;
        }
        if !marks.code && chars[i] == '=' && i + 1 < chars.len() && chars[i + 1] == '=' {
            flush(&mut runs, &mut buf, marks);
            marks.mark = !marks.mark;
            i += 2;
            continue;
        }
        if !marks.code && chars[i] == '~' && i + 1 < chars.len() && chars[i + 1] == '~' {
            flush(&mut runs, &mut buf, marks);
            marks.strike = !marks.strike;
            i += 2;
            continue;
        }
        if !marks.code && chars[i] == '*' && i + 1 < chars.len() && chars[i + 1] == '*' {
            flush(&mut runs, &mut buf, marks);
            marks.bold = !marks.bold;
            i += 2;
            continue;
        }
        if !marks.code && chars[i] == '_' && i + 1 < chars.len() && chars[i + 1] == '_' {
            flush(&mut runs, &mut buf, marks);
            marks.under = !marks.under;
            i += 2;
            continue;
        }
        if !marks.code && chars[i] == '^' {
            flush(&mut runs, &mut buf, marks);
            marks.super_ = !marks.super_;
            i += 1;
            continue;
        }
        if !marks.code && chars[i] == '~' {
            flush(&mut runs, &mut buf, marks);
            marks.sub_ = !marks.sub_;
            i += 1;
            continue;
        }
        if !marks.code && (chars[i] == '*' || chars[i] == '_') {
            flush(&mut runs, &mut buf, marks);
            marks.italic = !marks.italic;
            i += 1;
            continue;
        }
        buf.push(chars[i]);
        i += 1;
    }
    flush(&mut runs, &mut buf, marks);
    runs
}

fn decimal_item(text: &str, n: u32, nest: u8) -> Paragraph {
    let mut p = paragraph_from_inlines(text);
    p.space_after = LIST_ITEM_AFTER_HU;
    p.indent_left = list_indent(nest);
    p.numbering = Some(NumberingRef {
        definition_id: 1,
        level: nest,
        start: Some(n),
        format: Some(NumberFormat::Decimal),
    });
    p
}

fn task_item(text: &str) -> Option<(bool, &str)> {
    if let Some(rest) = text.strip_prefix("[ ] ") {
        return Some((false, rest));
    }
    text.strip_prefix("[x] ")
        .or_else(|| text.strip_prefix("[X] "))
        .map(|rest| (true, rest))
}

fn task_para(text: &str, nest: u8, checked: bool) -> Paragraph {
    let mut p = paragraph_from_inlines(text);
    p.space_after = LIST_ITEM_AFTER_HU;
    p.indent_left = list_indent(nest);
    p.numbering = Some(NumberingRef {
        definition_id: 2,
        level: nest,
        start: checked.then_some(1),
        format: Some(NumberFormat::Task),
    });
    p
}

fn bullet_item(text: &str, nest: u8) -> Paragraph {
    let mut p = paragraph_from_inlines(text);
    p.space_after = LIST_ITEM_AFTER_HU;
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
    p.line_spacing = LineSpacing::Percent(HEADING_LEADING_PCT);
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
            cell.padding = letter_cell_padding();
            for b in &mut cell.blocks {
                if let Block::Paragraph(p) = b {
                    let text = p.plain_text();
                    *p = paragraph_from_inlines(&text);
                }
            }
            if header {
                apply_letter_th_cell(cell);
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
        for (i, block) in section.body.iter().enumerate() {
            match block {
                Block::Paragraph(p) => {
                    let listed = p.numbering.is_some();
                    match p.outline_level {
                        Some(1) => out.push_str(&format!("# {}\n\n", p.plain_text())),
                        Some(2) => out.push_str(&format!("## {}\n\n", p.plain_text())),
                        _ if p
                            .numbering
                            .as_ref()
                            .is_some_and(|n| n.format == Some(NumberFormat::Decimal)) =>
                        {
                            let nref = p.numbering.as_ref().unwrap();
                            let i = nref.start.unwrap_or(1);
                            let pad = "  ".repeat(nref.level as usize);
                            out.push_str(&format!("{pad}{i}. {}\n", write_runs(p)));
                        }
                        _ if p
                            .numbering
                            .as_ref()
                            .is_some_and(|n| n.format == Some(NumberFormat::Task)) =>
                        {
                            let nref = p.numbering.as_ref().unwrap();
                            let pad = "  ".repeat(nref.level as usize);
                            let mark = if nref.start == Some(1) {
                                "- [x] "
                            } else {
                                "- [ ] "
                            };
                            out.push_str(&format!("{pad}{mark}{}\n", write_runs(p)));
                        }
                        _ if p.numbering.is_some() => {
                            let pad = "  ".repeat(
                                p.numbering.as_ref().map(|n| n.level as usize).unwrap_or(0),
                            );
                            out.push_str(&format!("{pad}- {}\n", write_runs(p)));
                        }
                        _ if p.quote => out.push_str(&format!("> {}\n\n", write_runs(p))),
                        _ if p.code_block => {
                            out.push_str("```\n");
                            out.push_str(&p.plain_text());
                            out.push_str("\n```\n\n");
                        }
                        _ => out.push_str(&format!("{}\n\n", write_runs(p))),
                    }
                    if listed {
                        let next_listed = section.body.get(i + 1).is_some_and(
                            |b| matches!(b, Block::Paragraph(q) if q.numbering.is_some()),
                        );
                        if !next_listed {
                            out.push('\n');
                        }
                    }
                }
                Block::Table(table) => {
                    for (ri, row) in table.rows.iter().enumerate() {
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
                        if ri == 0 {
                            let n = cells.len().max(1);
                            out.push('|');
                            out.push_str(&" --- |".repeat(n));
                            out.push('\n');
                        }
                    }
                    out.push('\n');
                }
                Block::Break(BreakKind::Thematic) => out.push_str("---\n\n"),
                Block::Float(docagent_model::Float::Image(img)) => {
                    out.push_str(&write_image(img));
                    out.push_str("\n\n");
                }
                Block::Float(_) | Block::Break(_) => {}
            }
        }
    }
    Ok(out.into_bytes())
}

fn parse_md_link_parts(chars: &[char], start: usize) -> Option<(String, String, usize)> {
    if start >= chars.len() || chars[start] != '[' {
        return None;
    }
    let mut j = start + 1;
    while j < chars.len() && chars[j] != ']' {
        j += 1;
    }
    if j + 1 >= chars.len() || chars[j] != ']' || chars[j + 1] != '(' {
        return None;
    }
    let display: String = chars[start + 1..j].iter().collect();
    let mut k = j + 2;
    while k < chars.len() && chars[k] != ')' {
        k += 1;
    }
    if k >= chars.len() || chars[k] != ')' {
        return None;
    }
    let target: String = chars[j + 2..k].iter().collect();
    Some((display, target, k + 1))
}

fn parse_md_link(chars: &[char], start: usize) -> Option<(String, String, usize)> {
    let (display, dest, next) = parse_md_link_parts(chars, start)?;
    let (target, _) = split_md_src_title(&dest);
    if display.is_empty() || target.is_empty() {
        return None;
    }
    Some((display, target, next))
}

fn parse_md_image(chars: &[char], start: usize) -> Option<(String, String, usize)> {
    if start >= chars.len() || chars[start] != '!' {
        return None;
    }
    let (alt, dest, next) = parse_md_link_parts(chars, start + 1)?;
    let (src, title) = split_md_src_title(&dest);
    if src.is_empty() {
        return None;
    }
    let alt = if alt.is_empty() {
        title.unwrap_or_default()
    } else {
        alt
    };
    Some((alt, src, next))
}

fn split_md_src_title(dest: &str) -> (String, Option<String>) {
    let dest = dest.trim();
    if dest.is_empty() {
        return (String::new(), None);
    }
    if let Some(rest) = dest.strip_prefix('<')
        && let Some(end) = rest.find('>')
    {
        let src = rest[..end].trim().to_string();
        let title = md_quoted_title(rest[end + 1..].trim());
        return (src, title);
    }
    let src_end = dest
        .char_indices()
        .find(|(_, c)| c.is_whitespace())
        .map(|(i, _)| i)
        .unwrap_or(dest.len());
    let src = dest[..src_end].to_string();
    (src, md_quoted_title(dest[src_end..].trim()))
}

fn md_quoted_title(rest: &str) -> Option<String> {
    let rest = rest.trim();
    let mut chars = rest.chars();
    let q = chars.next()?;
    if q != '"' && q != '\'' {
        return None;
    }
    let mut title = String::new();
    for c in chars {
        if c == q {
            return Some(title);
        }
        title.push(c);
    }
    None
}

fn image_run(alt: String, src: &str) -> Run {
    Run {
        style: docagent_model::CharStyle::default(),
        content: RunContent::Inline(InlineObject::Image(image_from_src(&alt, src))),
    }
}

fn image_from_src(alt: &str, src: &str) -> ImageData {
    let (mime, bytes) = decode_data_uri(src)
        .or_else(|| read_relative_image(src))
        .unwrap_or_else(|| (mime_from_name(src), Vec::new()));
    let (width, height) = raster_hu(&bytes);
    ImageData {
        bytes,
        mime,
        width,
        height,
        alt_text: if alt.is_empty() {
            None
        } else {
            Some(alt.to_string())
        },
        wrap: WrapMode::Inline,
    }
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

fn write_image(img: &ImageData) -> String {
    let alt = img.alt_text.as_deref().unwrap_or("");
    let mut s = String::from("![");
    s.push_str(alt);
    s.push_str("](");
    s.push_str(&write_image_src(img));
    s.push(')');
    s
}

/// Prefer the letter.md relative path when bytes match the fixture so
/// `![Harbor mark](docs/assets/mark.png)` round-trips instead of a data URI.
fn write_image_src(img: &ImageData) -> String {
    relative_path_for_bytes(&img.bytes).unwrap_or_else(|| image_src(img))
}

fn relative_path_for_bytes(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }
    let found = read_relative_bytes(LETTER_IMAGE_PATH)?;
    (found.as_slice() == bytes).then(|| LETTER_IMAGE_PATH.to_string())
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

fn png_px(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.len() < 24 || !bytes.starts_with(SIG) || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    (w > 0 && h > 0).then_some((w, h))
}

/// Smallest image edge (24pt), matching the PDF on-page floor.
/// 16px at 96dpi is 12pt and paints as a speck.
const MIN_IMAGE_EDGE_HU: i32 = 24 * docagent_model::HU_PER_POINT;
/// Letter.md harbor-mark path. Write this instead of a data URI when bytes match.
const LETTER_IMAGE_PATH: &str = "docs/assets/mark.png";
const _: () = assert!(LETTER_IMAGE_PATH.as_bytes()[0] == b'd');

/// Letter CSS `th,td{padding:.4rem .65rem}` at 16px rem / 96dpi: 0.65rem = 780 HU.
const CELL_PAD_X_HU: i32 = 780;
/// 0.4rem = 480 HU.
const CELL_PAD_Y_HU: i32 = 480;
/// Letter CSS `p{margin:0 0 .7rem}` collapsed with `img{margin:.6rem 0}` is 0.7rem.
const IMAGE_AFTER_HU: i32 = 840;
/// Letter CSS `h1{font-size:1.75rem}` at 16px rem / 96dpi: 1.75rem = 2100 HU.
/// Markdown 18pt (1800 HU) is the wrong web size for WeasyPrint input.
const H1_SIZE_HU: i32 = 2100;
/// Letter CSS `h2{font-size:1.15rem}`: 1.15rem = 1380 HU.
const H2_SIZE_HU: i32 = 1380;
/// WeasyPrint/CSS `line-height:1.45`. Hangul IR 160% is double-spaced on a letter page.
const BODY_LEADING_PCT: u16 = 145;
/// Heading leading, percent of font size (`h1,h2{line-height:1.2}`).
const HEADING_LEADING_PCT: u16 = 120;
/// Letter CSS `ul,ol{padding-left:1.25rem}` at 16px rem / 96dpi: 1.25rem = 1500 HU.
/// Hangul 1440 (1in) is the wrong web indent for WeasyPrint input.
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
/// Letter CSS `th{background:#ccfbf1}`.
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
/// Letter CSS `article{max-width:48rem}` — readable measure HTML wraps this IR in.
/// 100rem / full-bleed is not WeasyPrint letter input. Markdown has no page CSS.
const ARTICLE_MAX_WIDTH_REM: u8 = 48;
const _: () = assert!(ARTICLE_MAX_WIDTH_REM == 48);
/// Letter CSS `article{max-width:48rem;…;font-size:10pt;line-height:1.45;color:#134e4a}`.
/// Markdown has no page CSS; HTML WeasyPrint input wraps this IR in [`ARTICLE_MAX_WIDTH_REM`].
const ARTICLE_RULE: &str = r#"article{max-width:48rem;margin:2rem auto;padding:2.5rem 2.75rem;background:#fff;border:1px solid #e2e8f0;font-family:"Segoe UI",Roboto,Helvetica,Arial,sans-serif;font-size:10pt;line-height:1.45;color:#134e4a}"#;
/// Letter CSS `img` 24pt floor. WeasyPrint paints PNG intrinsic 16px (12pt) unless
/// min-* is 24pt **and** the HTML tag names pt. Markdown has no page CSS; IR
/// size stays [`MIN_IMAGE_EDGE_HU`].
const IMG_RULE: &str = "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}";
/// Letter CSS `@page{size:A4;margin:2.5rem 2.75rem}` matching `article{padding:2.5rem 2.75rem}`.
/// WeasyPrint UA `@page{margin:75px}` is not letter.html. Markdown has no page CSS.
const PAGE_RULE: &str = "@page{size:A4;margin:2.5rem 2.75rem}";
const _: () = assert!(PAGE_RULE.as_bytes()[0] == b'@');
/// Letter CSS `a{color:#0d9488;text-decoration:underline}`. Markdown has no page CSS.
const LINK_RULE: &str = "a{color:#0d9488;text-decoration:underline}";
/// Letter CSS `a:visited{color:#0d9488}` — keep rest teal, not UA purple. Markdown has no page CSS.
const LINK_VISITED_RULE: &str = "a:visited{color:#0d9488}";
/// Letter CSS `a:hover{color:#134e4a}` — rest teal like body/hr. Markdown has no page CSS.
const LINK_HOVER_RULE: &str = "a:hover{color:#134e4a}";
/// Letter CSS `a:focus{color:#134e4a}` — rest teal like hover/body/hr. Markdown has no page CSS.
const LINK_FOCUS_RULE: &str = "a:focus{color:#134e4a}";
/// Letter CSS `::selection` — UA Highlight (system blue) punches a hole in the teal chip.
/// Pin mint fill + body teal like `th`/`code`/`kbd`. Markdown has no page CSS.
const SELECTION_RULE: &str = "::selection{background:#ccfbf1;color:#134e4a}::-moz-selection{background:#ccfbf1;color:#134e4a}";
/// Letter CSS `h1`/`h2` bottom rules — WeasyPrint UA `h1{font-size:2em;margin:.67em 0}`
/// has no teal rule under the title. Markdown has no page CSS.
const HEADING_RULE: &str = "h1{font-size:1.75rem;line-height:1.2;margin:0 0 .6rem;letter-spacing:-.02em;padding-bottom:.4rem;border-bottom:3px solid #134e4a}h2{font-size:1.15rem;line-height:1.2;margin:1.1rem 0 .45rem;color:#134e4a;padding-bottom:.2rem;border-bottom:2px solid #0d9488}";
/// Letter CSS `abbr` — WeasyPrint UA dotted underline is uncolored CanvasText.
/// Markdown has no page CSS.
const ABBR_RULE: &str = "abbr[title],acronym[title]{text-decoration:underline dotted #134e4a;text-underline-offset:.15em;cursor:help}";
/// Letter CSS `details`/`summary` — HTML UA `summary{display:list-item}` paints a
/// disclosure ▸ that punches the 48rem column. Markdown has no page CSS.
const DETAILS_RULE: &str = "details{margin:0 0 .7rem;color:#134e4a}summary{display:block;cursor:pointer;font-weight:700;color:#134e4a;list-style:none}details[open]>summary{list-style:none}";
/// Letter CSS `tbody{display:table-row-group}` — `display:block` table resets stack hop
/// body rows as blocks. Markdown has no page CSS.
const TBODY_RULE: &str = "tbody{display:table-row-group}";
/// Letter CSS `table{border-collapse:collapse}` — WeasyPrint UA
/// `border-spacing:2px` punches 2px hop-cell gaps. Markdown has no page CSS.
const TABLE_RULE: &str = "table{border-collapse:collapse;margin:1rem 0;width:100%}";
/// Letter CSS `th,td{border:1px solid #cbd5e1;padding:.4rem .65rem}` —
/// WeasyPrint UA `td,th{padding:1px}` jams hop cells with no slate grid.
/// Markdown has no page CSS; IR pad stays [`CELL_PAD_X_HU`]/[`CELL_PAD_Y_HU`].
const CELL_RULE: &str = "th,td{border:1px solid #cbd5e1;padding:.4rem .65rem;text-align:left;font-size:.95rem}";
/// Letter CSS `ul,ol{margin:0 0 1rem;padding-left:1.25rem}` — WeasyPrint UA
/// `padding-left:40px` punches a 40px gutter in the 48rem column.
/// Markdown has no page CSS; IR indent stays [`LIST_INDENT_HU`].
const LIST_RULE: &str = "ul,ol{margin:0 0 1rem;padding-left:1.25rem}";
/// Letter CSS `li{margin:0 0 .35rem}` — WeasyPrint UA `li{display:list-item}`
/// has no after-margin, so agent-replay / task items jam. Markdown has no
/// page CSS; IR stays [`LIST_ITEM_AFTER_HU`].
const LI_RULE: &str = "li{margin:0 0 .35rem}";
/// Letter CSS `ul.tasks{list-style:none;padding-left:1.25rem}` — WeasyPrint UA
/// `ul{list-style:disc;padding-left:40px}` paints discs beside GFM checkboxes
/// and a 40px gutter. Markdown has no page CSS; IR stays `NumberFormat::Task`
/// at [`LIST_INDENT_HU`].
const TASKS_RULE: &str = "ul.tasks{list-style:none;padding-left:1.25rem}ul.tasks>li{list-style:none}ul.tasks ul{list-style:disc}ul.tasks input[type=checkbox]{appearance:none;-webkit-appearance:none;display:inline-block;width:.9em;height:.9em;margin-right:.4rem;vertical-align:middle;border:1.5pt solid #134e4a;border-radius:2px;background:#fff;accent-color:#134e4a}ul.tasks input[type=checkbox]:checked{background:#134e4a}";
/// Letter CSS `code{padding:.1em .35em}` chip. WeasyPrint UA `code` is bare
/// monospace with no mint pad, so `prove` paints as sparse body type.
/// Markdown has no page CSS; Consolas stays [`CODE_FONT`].
const CODE_RULE: &str = "code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}";
/// Letter CSS `pre{padding:.75rem 1rem}` mint box. WeasyPrint UA `pre` is
/// `margin:1em 0` with no pad/fill, so the prove fence sits flush and sparse.
/// Markdown has no page CSS; fenced IR spacing stays [`CODE_BEFORE_HU`]/[`CODE_AFTER_HU`].
const PRE_RULE: &str = "pre{background:#ccfbf1;padding:.75rem 1rem;margin:.6rem 0 1rem;border-radius:4px;overflow:auto;font-family:ui-monospace,Consolas,monospace}";
/// Letter CSS `pre code{background:transparent;padding:0}` — [`CODE_RULE`]
/// chips `prove`; without zeroing, that pad insets the prove fence inside
/// [`PRE_RULE`]'s `.75rem 1rem` box. Markdown has no page CSS; fenced IR
/// stays [`CODE_FONT`] under [`CODE_BEFORE_HU`]/[`CODE_AFTER_HU`].
const PRE_CODE_RULE: &str = "pre code{background:transparent;padding:0;color:#134e4a;font-family:ui-monospace,Consolas,monospace}";
/// Letter CSS `hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}` —
/// WeasyPrint UA `hr` is 1px inset / 0.5em gray. Markdown has no page CSS;
/// thematic IR stays [`BreakKind::Thematic`].
const HR_RULE: &str = "hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}";
/// Letter CSS `::marker{color:#134e4a}` — WeasyPrint UA `::marker` has no color,
/// so agent-replay decimals paint CanvasText black. Markdown has no page CSS;
/// list IR color stays [`BODY_FG`] / [`NumberFormat::Decimal`].
const MARKER_RULE: &str = "::marker{color:#134e4a}";
const _: () = assert!(LINK_RULE.as_bytes()[0] == b'a');
const _: () = assert!(LINK_VISITED_RULE.as_bytes()[0] == b'a');
const _: () = assert!(LINK_HOVER_RULE.as_bytes()[0] == b'a');
const _: () = assert!(LINK_FOCUS_RULE.as_bytes()[0] == b'a');
const _: () = assert!(SELECTION_RULE.as_bytes()[0] == b':');
const _: () = assert!(HEADING_RULE.as_bytes()[0] == b'h');
const _: () = assert!(ABBR_RULE.as_bytes()[0] == b'a');
const _: () = assert!(DETAILS_RULE.as_bytes()[0] == b'd');
const _: () = assert!(TBODY_RULE.as_bytes()[0] == b't');
const _: () = assert!(TABLE_RULE.as_bytes()[0] == b't');
const _: () = assert!(CELL_RULE.as_bytes()[0] == b't');
const _: () = assert!(LIST_RULE.as_bytes()[0] == b'u');
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

/// Letter CSS `th{background:#ccfbf1;color:#134e4a;font-weight:700}`.
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

fn px_to_hu(px: u32) -> i32 {
    i32::try_from(i64::from(px).saturating_mul(i64::from(docagent_model::HU_PER_INCH)) / 96)
        .unwrap_or(i32::MAX)
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

fn raster_hu(bytes: &[u8]) -> (docagent_model::Hu, docagent_model::Hu) {
    match png_px(bytes) {
        Some((w, h)) => min_on_page_size(px_to_hu(w), px_to_hu(h)),
        None => (0, 0),
    }
}

fn write_runs(p: &Paragraph) -> String {
    let mut s = String::new();
    for run in &p.runs {
        match &run.content {
            RunContent::Inline(InlineObject::Image(img)) => s.push_str(&write_image(img)),
            RunContent::Inline(InlineObject::Hyperlink { target, display }) => {
                s.push('[');
                s.push_str(display);
                s.push_str("](");
                s.push_str(target);
                s.push(')');
            }
            RunContent::Text(t) => {
                if t.is_empty() {
                    continue;
                }
                if run.style.bold {
                    s.push_str("**");
                }
                if run.style.underline != docagent_model::Underline::None {
                    s.push_str("__");
                }
                if run.style.italic {
                    s.push('_');
                }
                if run.style.strike {
                    s.push_str("~~");
                }
                if run.style.highlight.is_some() {
                    s.push_str("==");
                }
                if run.style.superscript {
                    s.push('^');
                }
                if run.style.subscript {
                    s.push('~');
                }
                if run.style.code {
                    s.push('`');
                }
                s.push_str(t);
                if run.style.code {
                    s.push('`');
                }
                if run.style.subscript {
                    s.push('~');
                }
                if run.style.superscript {
                    s.push('^');
                }
                if run.style.highlight.is_some() {
                    s.push_str("==");
                }
                if run.style.strike {
                    s.push_str("~~");
                }
                if run.style.italic {
                    s.push('_');
                }
                if run.style.underline != docagent_model::Underline::None {
                    s.push_str("__");
                }
                if run.style.bold {
                    s.push_str("**");
                }
            }
            RunContent::Inline(_) => {}
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
        let text = String::from_utf8(back).unwrap();
        assert!(text.contains("| --- | --- |"), "{text}");
    }

    #[test]
    fn table_write_emits_gfm_separator() {
        let src =
            b"| Hop | File | Proof |\n| --- | --- | --- |\n| Input | letter.md | input SHA-256 |\n";
        let doc = read(src).unwrap();
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(back.contains("| --- | --- | --- |"), "{back}");
        let again = read(back.as_bytes()).unwrap();
        assert_eq!(again.table_count(), 1);
        let Block::Table(t) = &again.sections[0].body[0] else {
            panic!("table");
        };
        assert_eq!(t.rows.len(), 2, "separator must not become a data row");
        assert!(t.rows[0].header);
        assert_eq!(t.rows[0].cells[0].shading, Some(TH_BG));
        assert_eq!(t.rows[1].cells[0].shading, None);
        assert!(again.plain_text().contains("letter.md"));
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
        assert!(h.runs.iter().any(|r| r.style.bold
            && r.style.color == TH_FG
            && matches!(&r.content, RunContent::Text(t) if t.contains("Hop"))));
        assert_eq!(t.rows[0].cells[0].shading, Some(TH_BG));
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
            r.style.italic
                && matches!(&r.content, RunContent::Text(t) if t.contains("byte-for-byte"))
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
        let text = String::from_utf8(back).unwrap();
        assert!(text.contains("- replay the convert\n\n"), "{text}");
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
            .map(|r| {
                (
                    r.style.bold,
                    r.style.italic,
                    match &r.content {
                        RunContent::Text(t) => t.as_str(),
                        _ => "",
                    },
                )
            })
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
    fn superscripts_roundtrip() {
        let src = b"the PDF/^A^ bytes match\n";
        let doc = read(src).unwrap();
        let Block::Paragraph(p) = &doc.sections[0].body[0] else {
            panic!("para");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.superscript && matches!(&r.content, RunContent::Text(t) if t == "A")
        }));
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(back.contains("PDF/^A^"), "{back}");
        assert!(!back.contains("^the"), "{back}");
    }

    #[test]
    fn subscripts_roundtrip() {
        let src = b"Wet cargo is H~2~O only.\n";
        let doc = read(src).unwrap();
        let Block::Paragraph(p) = &doc.sections[0].body[0] else {
            panic!("para");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.subscript && matches!(&r.content, RunContent::Text(t) if t == "2")
        }));
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(back.contains("H~2~O"), "{back}");
        assert!(!back.contains("~Wet"), "{back}");
        let strike = read(b"Agents ~~guess~~.\n").unwrap();
        let Block::Paragraph(p) = &strike.sections[0].body[0] else {
            panic!("para");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.strike
                && !r.style.subscript
                && matches!(&r.content, RunContent::Text(t) if t == "guess")
        }));
    }

    #[test]
    fn underlines_roundtrip() {
        let src = b"Please confirm before __09:00 local__:\n";
        let doc = read(src).unwrap();
        let Block::Paragraph(p) = &doc.sections[0].body[0] else {
            panic!("para");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.underline != docagent_model::Underline::None
                && matches!(&r.content, RunContent::Text(t) if t == "09:00 local")
        }));
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(back.contains("__09:00 local__"), "{back}");
        assert!(!back.contains("__Please"), "{back}");
    }

    #[test]
    fn highlights_roundtrip() {
        let src = b"one convert, ==three hashes==, GPU-free\n";
        let doc = read(src).unwrap();
        let Block::Paragraph(p) = &doc.sections[0].body[0] else {
            panic!("para");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.highlight == Some(docagent_model::Color::MARK)
                && matches!(&r.content, RunContent::Text(t) if t == "three hashes")
        }));
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(back.contains("==three hashes=="), "{back}");
        assert!(!back.contains("==one"), "{back}");
    }

    #[test]
    fn task_items_roundtrip() {
        let src = b"- [x] Replay the convert\n- [ ] Hope\n";
        let doc = read(src).unwrap();
        let flags: Vec<_> = doc.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => p
                    .numbering
                    .as_ref()
                    .map(|n| (n.format, n.start, p.plain_text())),
                _ => None,
            })
            .collect();
        assert_eq!(
            flags,
            [
                (
                    Some(NumberFormat::Task),
                    Some(1),
                    "Replay the convert".into()
                ),
                (Some(NumberFormat::Task), None, "Hope".into()),
            ]
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(back.contains("- [x] Replay the convert"), "{back}");
        assert!(back.contains("- [ ] Hope"), "{back}");
        assert_eq!(
            doc.sections[0]
                .body
                .iter()
                .filter_map(|b| match b {
                    Block::Paragraph(p) if p.numbering.is_some() => Some(p.indent_left),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            [LIST_INDENT_HU, LIST_INDENT_HU]
        );
        assert_eq!(
            match &doc.sections[0].body[0] {
                Block::Paragraph(p) => p.space_after,
                _ => panic!("task"),
            },
            LIST_ITEM_AFTER_HU
        );
        assert_eq!(
            match &doc.sections[0].body[1] {
                Block::Paragraph(p) => p.space_after,
                _ => panic!("task"),
            },
            LIST_AFTER_HU
        );
    }

    #[test]
    fn letter_md_task_items() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("- [x] Receiving dock is clear"),
            "letter.md must keep the checked GFM task"
        );
        assert!(
            text.contains("- [ ] Replay the convert instead of hoping"),
            "letter.md must keep the unchecked GFM task"
        );
        let doc = read(&md).expect("docagent-md read letter.md");
        let tasks: Vec<_> = doc.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => p.numbering.as_ref().and_then(|n| {
                    (n.format == Some(NumberFormat::Task))
                        .then(|| (p.plain_text(), n.start, p.indent_left))
                }),
                _ => None,
            })
            .collect();
        assert!(
            tasks.iter().any(|(t, start, indent)| {
                t.contains("Receiving dock") && *start == Some(1) && *indent == LIST_INDENT_HU
            }),
            "letter.md - [x] Receiving dock must stay NumberFormat::Task checked: {tasks:?}"
        );
        assert!(
            tasks.iter().any(|(t, start, indent)| {
                t.contains("Capsule hashes") && *start == Some(1) && *indent == LIST_INDENT_HU
            }),
            "letter.md - [x] Capsule hashes must stay NumberFormat::Task checked: {tasks:?}"
        );
        assert!(
            tasks.iter().any(|(t, start, indent)| {
                t.contains("Replay the convert") && start.is_none() && *indent == LIST_INDENT_HU
            }),
            "letter.md - [ ] Replay must stay NumberFormat::Task unchecked: {tasks:?}"
        );
        assert!(
            !tasks.iter().any(|(t, _, _)| t.contains("Bay 4")),
            "nested Bay 4 under a task is a bullet, not a checkbox: {tasks:?}"
        );
        let nested = doc.sections[0].body.iter().find_map(|b| match b {
            Block::Paragraph(p)
                if p.plain_text().contains("Bay 4")
                    && p.numbering
                        .as_ref()
                        .is_some_and(|n| n.format == Some(NumberFormat::Bullet)) =>
            {
                Some(p)
            }
            _ => None,
        });
        assert!(
            nested.is_some(),
            "Bay 4 must stay a nested bullet under the task"
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(back.contains("- [x] Receiving dock is clear"), "{back}");
        assert!(
            back.contains("- [x] Capsule hashes travel with the files"),
            "{back}"
        );
        assert!(
            back.contains("- [ ] Replay the convert instead of hoping"),
            "{back}"
        );
        assert!(
            first_image(&doc).width >= MIN_IMAGE_EDGE_HU
                && first_image(&doc).height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let table = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Table(t) => Some(t),
                _ => None,
            })
            .expect("letter table");
        assert_eq!(table.rows[0].cells[0].padding.left, CELL_PAD_X_HU);
        assert_eq!(
            table.rows[0].cells[0].shading,
            Some(TH_BG),
            "letter.md header cells must keep #ccfbf1 fill"
        );
        assert!(
            doc.sections[0]
                .body
                .iter()
                .any(|b| matches!(b, Block::Break(BreakKind::Thematic))),
            "hr must not regress"
        );
        assert!(
            doc.sections[0].body.iter().any(|b| match b {
                Block::Paragraph(p) => p.runs.iter().any(|r| {
                    r.style.code && matches!(&r.content, RunContent::Text(t) if t == "prove")
                }),
                _ => false,
            }),
            "inline code must not regress"
        );
        assert!(
            doc.sections[0].body.iter().any(|b| match b {
                Block::Paragraph(p) => p.runs.iter().any(|r| {
                    r.style.highlight == Some(docagent_model::Color::MARK)
                        && matches!(&r.content, RunContent::Text(t) if t == "three hashes")
                }),
                _ => false,
            }),
            "mark highlight must not regress"
        );
        assert!(
            doc.sections[0].body.iter().any(|b| match b {
                Block::Paragraph(p) => p.runs.iter().any(|r| matches!(
                    &r.content,
                    RunContent::Inline(InlineObject::Hyperlink { display, .. })
                        if display == "DocAgent"
                )),
                _ => false,
            }),
            "link must not regress"
        );
    }

    #[test]
    fn code_blocks_roundtrip() {
        let src = b"before\n\n```\ndocagent prove examples/letter.md\n```\n\nafter\n";
        let doc = read(src).unwrap();
        let block = doc.sections[0]
            .body
            .iter()
            .find(|b| matches!(b, Block::Paragraph(p) if p.code_block));
        let Some(Block::Paragraph(p)) = block else {
            panic!("code block");
        };
        assert!(p.plain_text().contains("docagent prove"));
        assert!(!p.plain_text().contains("```"));
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(
            back.contains("```\ndocagent prove examples/letter.md\n```"),
            "{back}"
        );
    }

    #[test]
    fn thematic_breaks_roundtrip() {
        let src = b"before\n\n---\n\nafter\n";
        let doc = read(src).unwrap();
        assert!(
            doc.sections[0]
                .body
                .iter()
                .any(|b| matches!(b, Block::Break(BreakKind::Thematic)))
        );
        assert!(doc.plain_text().contains("before"));
        assert!(doc.plain_text().contains("after"));
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(back.contains("---\n"), "{back}");
    }

    #[test]
    fn code_spans_roundtrip() {
        let src = b"Run `prove` twice\n";
        let doc = read(src).unwrap();
        let Block::Paragraph(p) = &doc.sections[0].body[0] else {
            panic!("para");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.code && matches!(&r.content, RunContent::Text(t) if t == "prove")
        }));
        assert!(p.runs.iter().any(|r| {
            !r.style.code && matches!(&r.content, RunContent::Text(t) if t.contains("Run"))
        }));
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(back.contains("`prove`"), "{back}");
        assert!(!back.contains("`Run"), "{back}");
    }

    #[test]
    fn quotes_roundtrip() {
        let src = b"> Replay is evidence. Same fonts, same bytes.\n";
        let doc = read(src).unwrap();
        let Block::Paragraph(p) = &doc.sections[0].body[0] else {
            panic!("para");
        };
        assert!(p.quote);
        assert!(p.plain_text().contains("Replay is evidence"));
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(back.starts_with("> Replay is evidence"), "{back}");
    }

    #[test]
    fn strikethrough_roundtrips() {
        let src = b"Agents ~~guess~~.\n";
        let doc = read(src).unwrap();
        let Block::Paragraph(p) = &doc.sections[0].body[0] else {
            panic!("para");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.strike && matches!(&r.content, RunContent::Text(t) if t.contains("guess"))
        }));
        assert!(p.runs.iter().any(|r| {
            !r.style.strike && matches!(&r.content, RunContent::Text(t) if t.contains("Agents"))
        }));
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(back.contains("~~guess~~"), "{back}");
        assert!(!back.contains("~~Agents"), "{back}");
    }

    #[test]
    fn hyperlinks_roundtrip() {
        let src = b"[DocAgent](https://github.com/kevin9327/docagent) runtime\n";
        let doc = read(src).unwrap();
        let Block::Paragraph(p) = &doc.sections[0].body[0] else {
            panic!("para");
        };
        assert!(p.runs.iter().any(|r| matches!(
            &r.content,
            RunContent::Inline(InlineObject::Hyperlink { target, display })
                if target == "https://github.com/kevin9327/docagent" && display == "DocAgent"
        )));
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(
            back.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "{back}"
        );
        assert!(
            p.runs.iter().any(|r| {
                r.style.underline == docagent_model::Underline::Single
                    && matches!(
                        &r.content,
                        RunContent::Inline(InlineObject::Hyperlink { .. })
                    )
            }),
            "markdown links must keep the WeasyPrint underline"
        );
        let titled =
            read(b"[DocAgent](https://github.com/kevin9327/docagent \"runtime\")\n").unwrap();
        let Block::Paragraph(p) = &titled.sections[0].body[0] else {
            panic!("para");
        };
        assert!(p.runs.iter().any(|r| matches!(
            &r.content,
            RunContent::Inline(InlineObject::Hyperlink { target, display })
                if target == "https://github.com/kevin9327/docagent" && display == "DocAgent"
        )));
    }

    fn fixture_png() -> Vec<u8> {
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/assets/mark.png"),
        )
        .expect("docs/assets/mark.png")
    }

    fn fixture_image(alt: &str) -> ImageData {
        let bytes = fixture_png();
        let (width, height) = raster_hu(&bytes);
        ImageData {
            bytes,
            mime: "image/png".into(),
            width,
            height,
            alt_text: Some(alt.into()),
            wrap: WrapMode::Inline,
        }
    }

    fn first_image(doc: &Document) -> &ImageData {
        for block in &doc.sections[0].body {
            match block {
                Block::Paragraph(p) => {
                    for run in &p.runs {
                        if let RunContent::Inline(InlineObject::Image(img)) = &run.content {
                            return img;
                        }
                    }
                }
                Block::Float(docagent_model::Float::Image(img)) => return img,
                _ => {}
            }
        }
        panic!("no image");
    }

    #[test]
    fn images_parse_bang_syntax() {
        let img = fixture_image("mark");
        let src = format!("![mark]({})\n", image_src(&img));
        let doc = read(src.as_bytes()).unwrap();
        let Block::Paragraph(p) = &doc.sections[0].body[0] else {
            panic!("para");
        };
        assert!(
            p.runs
                .iter()
                .any(|r| matches!(&r.content, RunContent::Inline(InlineObject::Image(_))))
        );
        assert_eq!(first_image(&doc), &img);
        assert!(p.runs.iter().all(|r| !matches!(
            &r.content,
            RunContent::Inline(InlineObject::Hyperlink { .. })
        )));
    }

    #[test]
    fn images_roundtrip_imagedata() {
        let img = fixture_image("mark");
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![Run {
            style: docagent_model::CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(img.clone())),
        }];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let back = write(&doc).unwrap();
        let text = String::from_utf8(back.clone()).unwrap();
        assert!(
            text.contains("![mark](docs/assets/mark.png)"),
            "fixture bytes must write the letter.md relative path, not a data URI: {text}"
        );
        assert!(
            !text.contains("![mark](data:image/png;base64,"),
            "path round-trip must not collapse to a data URI: {text}"
        );
        let again = read(&back).unwrap();
        assert_eq!(first_image(&again), &img);
        let twice = write(&again).unwrap();
        assert_eq!(first_image(&read(&twice).unwrap()), &img);
        assert!(
            String::from_utf8(twice).unwrap().contains("![mark](docs/assets/mark.png)"),
            "second write must keep the relative path"
        );
    }

    #[test]
    fn images_parse_title_without_breaking_src() {
        let img = fixture_image("mark");
        let src = format!("![mark]({} \"Harbor mark\")\n", image_src(&img));
        let doc = read(src.as_bytes()).unwrap();
        let got = first_image(&doc);
        assert_eq!(got.bytes, img.bytes);
        assert_eq!(got.mime, "image/png");
        assert_eq!(got.alt_text.as_deref(), Some("mark"));
        assert_eq!(got.wrap, WrapMode::Inline);
        let untitled = format!("![mark]({})\n", image_src(&img));
        assert_eq!(
            first_image(&read(untitled.as_bytes()).unwrap()).bytes,
            img.bytes
        );
        let titled = format!("![]({} 'caption')\n", image_src(&img));
        assert_eq!(
            first_image(&read(titled.as_bytes()).unwrap())
                .alt_text
                .as_deref(),
            Some("caption")
        );
    }

    #[test]
    fn images_parse_relative_path() {
        let png = fixture_png();
        let doc = read(b"![mark](docs/assets/mark.png)\n").unwrap();
        let img = first_image(&doc);
        assert_eq!(img.bytes, png);
        assert!(!img.bytes.is_empty());
        assert_eq!(img.mime, "image/png");
        assert_eq!(img.alt_text.as_deref(), Some("mark"));
        assert_eq!(img.wrap, WrapMode::Inline);
        let (width, height) = raster_hu(&png);
        assert_eq!((img.width, img.height), (width, height));
        assert_ne!((img.width, img.height), (0, 0));
        let titled = read(b"![mark](docs/assets/mark.png \"Harbor mark\")\n").unwrap();
        assert_eq!(first_image(&titled).bytes, png);
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(
            back.contains("![mark](docs/assets/mark.png)"),
            "relative path must round-trip: {back}"
        );
        assert!(
            !back.contains("![mark](data:image/png;base64,"),
            "letter.md path must not rewrite to a data URI: {back}"
        );
        assert_eq!(first_image(&read(back.as_bytes()).unwrap()).bytes, png);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
    }

    fn first_image_paragraph(doc: &Document) -> &Paragraph {
        for block in &doc.sections[0].body {
            if let Block::Paragraph(p) = block
                && p.runs
                    .iter()
                    .any(|r| matches!(&r.content, RunContent::Inline(InlineObject::Image(_))))
            {
                return p;
            }
        }
        panic!("no image paragraph");
    }

    #[test]
    fn letter_md_reads_mark_png() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        if !text.contains("![") {
            return;
        }
        let doc = read(&md).expect("docagent-md read letter.md");
        let png = fixture_png();
        let img = first_image(&doc);
        assert_eq!(img.bytes, png);
        assert!(!img.bytes.is_empty());
        assert_eq!(
            (img.width, img.height),
            (MIN_IMAGE_EDGE_HU, MIN_IMAGE_EDGE_HU),
            "letter.md harbor mark IR must be the PDF 24pt floor, not 16px at 96dpi ({}×{})",
            img.width,
            img.height
        );
    }

    #[test]
    fn table_cells_use_weasyprint_padding() {
        let src =
            b"| Hop | File | Proof |\n| --- | --- | --- |\n| Input | letter.md | input SHA-256 |\n";
        let doc = read(src).unwrap();
        let Block::Table(t) = &doc.sections[0].body[0] else {
            panic!("table");
        };
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
                assert!(
                    cell.padding.left > 100 && cell.padding.top > 100,
                    "sparse-grid UA padding is not WeasyPrint article input"
                );
            }
        }
        for cell in &t.rows[0].cells {
            assert_eq!(
                cell.shading,
                Some(TH_BG),
                "header cell must keep letter CSS th background #ccfbf1, not a sparse grid: {:?}",
                cell.shading
            );
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
        for row in t.rows.iter().skip(1) {
            for cell in &row.cells {
                assert_ne!(
                    cell.shading,
                    Some(TH_BG),
                    "body cells must not keep th fill"
                );
            }
        }
    }

    #[test]
    fn table_headers_use_weasyprint_fill() {
        let src =
            b"| Hop | File | Proof |\n| --- | --- | --- |\n| Input | letter.md | input SHA-256 |\n";
        let doc = read(src).unwrap();
        let Block::Table(t) = &doc.sections[0].body[0] else {
            panic!("table");
        };
        assert!(t.rows[0].header);
        assert_eq!(t.header_row_count, 1);
        assert_eq!(
            TH_BG,
            Color {
                r: 204,
                g: 251,
                b: 241,
                a: 255
            }
        );
        assert_eq!(
            TH_FG,
            Color {
                r: 19,
                g: 78,
                b: 74,
                a: 255
            }
        );
        for cell in &t.rows[0].cells {
            assert_eq!(
                cell.shading,
                Some(TH_BG),
                "header cell must keep letter CSS th background #ccfbf1, not a sparse grid: {:?}",
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
                "th text must stay bold #134e4a"
            );
        }
        for row in t.rows.iter().skip(1) {
            assert!(!row.header);
            for cell in &row.cells {
                assert_ne!(
                    cell.shading,
                    Some(TH_BG),
                    "body cells must not keep th fill"
                );
            }
        }
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(
            back.contains("| **Hop** | **File** | **Proof** |"),
            "{back}"
        );
        let again = read(back.as_bytes()).unwrap();
        let Block::Table(t2) = &again.sections[0].body[0] else {
            panic!("table");
        };
        assert_eq!(t2.rows[0].cells[0].shading, Some(TH_BG));
    }

    #[test]
    fn letter_md_table_header_fill() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("| Hop |"),
            "letter.md must contain the hop table"
        );
        let doc = read(&md).expect("docagent-md read letter.md");
        let table = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Table(t) => Some(t),
                _ => None,
            })
            .expect("letter table");
        assert!(table.rows[0].header);
        assert_eq!(table.header_row_count, 1);
        for cell in &table.rows[0].cells {
            assert_eq!(
                cell.shading,
                Some(TH_BG),
                "letter.md header cells must keep #ccfbf1 fill, not a sparse grid"
            );
            assert_eq!(cell.padding.left, CELL_PAD_X_HU);
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
        for row in table.rows.iter().skip(1) {
            for cell in &row.cells {
                assert_ne!(cell.shading, Some(TH_BG));
            }
        }
        let img = first_image(&doc);
        assert!(img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU);
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
        assert!(
            doc.sections[0].body.iter().any(|b| match b {
                Block::Paragraph(p) => p.runs.iter().any(|r| {
                    r.style.code && matches!(&r.content, RunContent::Text(t) if t == "prove")
                }),
                _ => false,
            }),
            "inline code must not regress"
        );
        assert!(
            doc.sections[0].body.iter().any(|b| match b {
                Block::Paragraph(p) => p.runs.iter().any(|r| {
                    r.style.highlight == Some(docagent_model::Color::MARK)
                        && matches!(&r.content, RunContent::Text(t) if t == "three hashes")
                }),
                _ => false,
            }),
            "mark highlight must not regress"
        );
    }

    #[test]
    fn image_only_paragraph_gets_post_mark_gap() {
        let doc = read(b"![Harbor mark](docs/assets/mark.png)\n\n6 September 2026\n").unwrap();
        let img_p = first_image_paragraph(&doc);
        assert!(paragraph_is_image_only(img_p));
        assert_eq!(
            img_p.space_after, IMAGE_AFTER_HU,
            "gap after mark {} must be letter CSS 0.7rem, not IR space_after 200",
            img_p.space_after
        );
        assert!(
            img_p.space_after > 200,
            "post-mark gap must not keep body space_after 200"
        );
        let Block::Paragraph(body) = &doc.sections[0].body[1] else {
            panic!("date");
        };
        assert_eq!(body.plain_text(), "6 September 2026");
        assert_eq!(body.space_after, 200);
        let img = first_image(&doc);
        assert!(img.width >= MIN_IMAGE_EDGE_HU);
        assert!(img.height >= MIN_IMAGE_EDGE_HU);
    }

    #[test]
    fn letter_md_table_padding_and_mark_gap() {
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
        let doc = read(&md).expect("docagent-md read letter.md");
        let table = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Table(t) => Some(t),
                _ => None,
            })
            .expect("letter table");
        let cell = &table.rows[0].cells[0];
        assert_eq!(cell.padding.left, CELL_PAD_X_HU);
        assert_eq!(cell.padding.top, CELL_PAD_Y_HU);
        assert_eq!(cell.shading, Some(TH_BG));
        let img_p = first_image_paragraph(&doc);
        assert_eq!(
            img_p.space_after, IMAGE_AFTER_HU,
            "letter.md harbor mark must keep a 0.7rem gap so the page is not empty under the image"
        );
        let img = first_image(&doc);
        assert!(img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU);
        assert_eq!(img.alt_text.as_deref(), Some("Harbor mark"));
    }

    #[test]
    fn min_on_page_size_raises_16px_to_24pt() {
        assert_eq!(
            min_on_page_size(1200, 1200),
            (MIN_IMAGE_EDGE_HU, MIN_IMAGE_EDGE_HU)
        );
        assert_eq!(min_on_page_size(2400, 2400), (2400, 2400));
        assert_eq!(min_on_page_size(4800, 2400), (4800, 2400));
        let native = px_to_hu(16);
        assert!(
            native < MIN_IMAGE_EDGE_HU,
            "16px at 96dpi must be below the 24pt floor ({native})"
        );
        let png = fixture_png();
        let (px_w, px_h) = png_px(&png).expect("mark.png IHDR");
        assert_eq!((px_w, px_h), (16, 16), "fixture is 16×16 px");
        let (w, h) = raster_hu(&png);
        assert!(w >= MIN_IMAGE_EDGE_HU);
        assert!(h >= MIN_IMAGE_EDGE_HU);
        assert!(w >= docagent_model::HU_PER_INCH / 4);
        assert!(h >= docagent_model::HU_PER_INCH / 4);
        let doc = read(b"![Harbor mark](docs/assets/mark.png)\n").unwrap();
        let img = first_image(&doc);
        assert_eq!(img.bytes, png);
        assert!(img.width >= MIN_IMAGE_EDGE_HU);
        assert!(img.height >= MIN_IMAGE_EDGE_HU);
    }

    #[test]
    fn headings_carry_size_for_layout() {
        let doc = read(b"# Title\n\n## Sub\n\nBody\n").unwrap();
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
        assert_eq!(
            h1.runs[0].style.size, 2100,
            "h1 must stay letter CSS 1.75rem"
        );
        assert_ne!(
            h1.runs[0].style.size, 1800,
            "Markdown 18pt is the wrong web size for WeasyPrint input"
        );
        assert!(h1.runs[0].style.bold);
        assert_eq!(h1.line_spacing, LineSpacing::Percent(HEADING_LEADING_PCT));
        assert_eq!(h2.outline_level, Some(2));
        assert_eq!(h2.runs[0].style.size, H2_SIZE_HU);
        assert_eq!(
            h2.runs[0].style.size, 1380,
            "h2 must stay letter CSS 1.15rem"
        );
        assert_ne!(
            h2.runs[0].style.size, 1400,
            "Markdown 14pt is the wrong web size for WeasyPrint input"
        );
        assert!(h2.runs[0].style.bold);
        assert_eq!(h2.line_spacing, LineSpacing::Percent(HEADING_LEADING_PCT));
        assert_eq!(body.outline_level, None);
        assert_eq!(body.line_spacing, LineSpacing::Percent(BODY_LEADING_PCT));
        assert_eq!(body.line_spacing, LineSpacing::Percent(145));
        assert_ne!(
            body.line_spacing,
            LineSpacing::Percent(160),
            "body must not keep Hangul 160% leading on a letter page"
        );
    }

    #[test]
    fn letter_md_heading_size_line_height_pad_and_img_floor() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = read(&md).expect("docagent-md read letter.md");
        let h1 = match &doc.sections[0].body[0] {
            Block::Paragraph(p) => p,
            _ => panic!("h1"),
        };
        let h2 = match &doc.sections[0].body[1] {
            Block::Paragraph(p) => p,
            _ => panic!("h2"),
        };
        assert_eq!(h1.plain_text(), "Northwind Freight");
        assert_eq!(h1.runs[0].style.size, H1_SIZE_HU);
        assert_eq!(h1.line_spacing, LineSpacing::Percent(HEADING_LEADING_PCT));
        assert_eq!(h2.plain_text(), "Delivery confirmation — order 48291");
        assert_eq!(h2.runs[0].style.size, H2_SIZE_HU);
        assert_eq!(h2.line_spacing, LineSpacing::Percent(HEADING_LEADING_PCT));
        let img = first_image(&doc);
        assert!(img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU);
        let img_p = first_image_paragraph(&doc);
        assert_eq!(img_p.space_after, IMAGE_AFTER_HU);
        let table = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Table(t) => Some(t),
                _ => None,
            })
            .expect("letter table");
        assert_eq!(table.rows[0].cells[0].padding.left, CELL_PAD_X_HU);
        assert_eq!(table.rows[0].cells[0].padding.top, CELL_PAD_Y_HU);
        assert_eq!(table.rows[0].cells[0].shading, Some(TH_BG));
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
        assert_eq!(body.line_spacing, LineSpacing::Percent(BODY_LEADING_PCT));
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
            "letter.md quote text must stay #134e4a like the WeasyPrint bar"
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
        let items: Vec<_> = doc.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) if p.numbering.is_some() => Some(p),
                _ => None,
            })
            .collect();
        assert!(!items.is_empty());
        assert_eq!(items[0].indent_left, LIST_INDENT_HU);
        assert_eq!(items[0].space_after, LIST_ITEM_AFTER_HU);
        assert_eq!(items.last().unwrap().space_after, LIST_AFTER_HU);
    }

    #[test]
    fn lists_use_weasyprint_indent_and_spacing() {
        let src = b"- dock is clear\n  - bay 4\n- replay the convert\n\nBody after.\n";
        let doc = read(src).unwrap();
        let items: Vec<_> = doc.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) if p.numbering.is_some() => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].indent_left, LIST_INDENT_HU);
        assert_eq!(
            items[0].indent_left, 1500,
            "list indent must stay letter CSS 1.25rem"
        );
        assert_ne!(
            items[0].indent_left, 1440,
            "Hangul 1in list indent is not WeasyPrint 1.25rem"
        );
        assert_eq!(items[0].space_after, LIST_ITEM_AFTER_HU);
        assert_eq!(
            items[0].space_after, 420,
            "li gap must stay letter CSS 0.35rem"
        );
        assert_ne!(
            items[0].space_after, 80,
            "IR list space_after 80 is jammed vs letter CSS 0.35rem"
        );
        assert_eq!(items[1].indent_left, LIST_INDENT_HU * 2);
        assert_eq!(items[1].space_after, LIST_ITEM_AFTER_HU);
        assert_eq!(items[2].indent_left, LIST_INDENT_HU);
        assert_eq!(items[2].space_after, LIST_AFTER_HU);
        assert_eq!(
            items[2].space_after, 1200,
            "last li must keep ul margin 1rem"
        );
        assert_ne!(items[2].space_after, 80);
        assert_ne!(items[2].space_after, 200);
        let ordered = read(b"1. Hash the input\n2. Run Convert\n").unwrap();
        let nums: Vec<_> = ordered.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) if p.numbering.is_some() => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(nums[0].indent_left, LIST_INDENT_HU);
        assert_eq!(nums[0].space_after, LIST_ITEM_AFTER_HU);
        assert_eq!(nums[1].space_after, LIST_AFTER_HU);
    }

    #[test]
    fn letter_md_thematic_break_and_hyperlink() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("\n---\n"),
            "letter.md must keep the thematic break that HTML emits as <hr>"
        );
        assert!(
            text.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "letter.md must keep the DocAgent hyperlink"
        );
        let doc = read(&md).expect("docagent-md read letter.md");
        assert!(
            doc.sections[0]
                .body
                .iter()
                .any(|b| matches!(b, Block::Break(BreakKind::Thematic))),
            "letter.md IR must keep BreakKind::Thematic"
        );
        let link = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) => p.runs.iter().find(|r| {
                    matches!(
                        &r.content,
                        RunContent::Inline(InlineObject::Hyperlink { target, display })
                            if target == "https://github.com/kevin9327/docagent"
                                && display == "DocAgent"
                    )
                }),
                _ => None,
            })
            .expect("letter hyperlink");
        assert_eq!(link.style.underline, docagent_model::Underline::Single);
        let img = first_image(&doc);
        assert!(img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU);
        let table = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Table(t) => Some(t),
                _ => None,
            })
            .expect("letter table");
        assert_eq!(table.rows[0].cells[0].padding.left, CELL_PAD_X_HU);
        let h1 = match &doc.sections[0].body[0] {
            Block::Paragraph(p) => p,
            _ => panic!("h1"),
        };
        assert_eq!(h1.runs[0].style.size, H1_SIZE_HU);
        let items: Vec<_> = doc.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) if p.numbering.is_some() => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(items[0].indent_left, LIST_INDENT_HU);
        let quote = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) if p.quote => Some(p),
                _ => None,
            })
            .expect("quote");
        assert_eq!(quote.space_before, QUOTE_BEFORE_HU);
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(back.contains("---\n"), "{back}");
        assert!(
            back.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "{back}"
        );
    }

    #[test]
    fn quotes_use_weasyprint_spacing() {
        let doc = read(b"> Replay is evidence. Same fonts, same bytes.\n").unwrap();
        let Block::Paragraph(p) = &doc.sections[0].body[0] else {
            panic!("para");
        };
        assert!(p.quote);
        assert_eq!(p.space_before, QUOTE_BEFORE_HU);
        assert_eq!(p.space_before, 600, "quote top must stay letter CSS 0.5rem");
        assert_ne!(
            p.space_before, 80,
            "IR quote space_before 80 is jammed vs letter CSS 0.5rem"
        );
        assert_eq!(p.space_after, QUOTE_AFTER_HU);
        assert_eq!(
            p.space_after, 1200,
            "quote bottom must stay letter CSS 1rem"
        );
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
            "quote text must stay letter CSS #134e4a (same teal as the 4px bar)"
        );
        assert_eq!(QUOTE_BAR_W_HU, 300, "4px bar at 96dpi is 300 HU");
        assert_eq!(QUOTE_FG, TH_FG);
    }

    #[test]
    fn code_blocks_use_weasyprint_spacing() {
        let doc = read(b"```\ndocagent prove examples/letter.md\n```\n").unwrap();
        let Block::Paragraph(p) = &doc.sections[0].body[0] else {
            panic!("para");
        };
        assert!(p.code_block);
        assert_eq!(p.space_before, CODE_BEFORE_HU);
        assert_eq!(p.space_before, 720, "pre top must stay letter CSS 0.6rem");
        assert_ne!(
            p.space_before, 80,
            "IR pre space_before 80 is jammed vs letter CSS 0.6rem"
        );
        assert_eq!(p.space_after, CODE_AFTER_HU);
        assert_eq!(p.space_after, 1200, "pre bottom must stay letter CSS 1rem");
        assert_ne!(
            p.space_after, 200,
            "IR pre space_after 200 is jammed vs letter CSS 1rem"
        );
        assert_eq!(p.line_spacing, LineSpacing::Percent(BODY_LEADING_PCT));
        assert!(
            p.runs.iter().any(|r| {
                r.style.font.as_deref() == Some(CODE_FONT)
                    && r.style.latin_font.as_deref() == Some(CODE_FONT)
            }),
            "pre must use letter CSS Consolas, not generic serif: {:?}",
            p.runs
                .iter()
                .map(|r| r.style.font.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(CODE_FONT, "Consolas");
        assert!(
            !p.runs.iter().any(|r| {
                matches!(
                    r.style.font.as_deref(),
                    Some("serif") | Some("Times") | Some("Times New Roman") | Some("Georgia")
                )
            }),
            "pre must not fall back to generic serif"
        );
    }

    #[test]
    fn code_uses_weasyprint_consolas_font() {
        let inline = read(b"Run `prove` twice.\n").unwrap();
        let Block::Paragraph(p) = &inline.sections[0].body[0] else {
            panic!("para");
        };
        let code = p
            .runs
            .iter()
            .find(|r| r.style.code && matches!(&r.content, RunContent::Text(t) if t == "prove"))
            .expect("inline code");
        assert_eq!(code.style.font.as_deref(), Some(CODE_FONT));
        assert_eq!(code.style.latin_font.as_deref(), Some(CODE_FONT));
        assert_eq!(CODE_FONT, "Consolas");
        assert_ne!(
            code.style.font.as_deref(),
            Some("serif"),
            "inline code must not inherit article serif"
        );
        assert_ne!(code.style.font.as_deref(), Some("Times New Roman"));
        let body = p
            .runs
            .iter()
            .find(|r| {
                !r.style.code && matches!(&r.content, RunContent::Text(t) if t.contains("Run"))
            })
            .expect("body run");
        assert!(
            body.style.font.is_none(),
            "body text must not pick up the Consolas chip font"
        );
    }

    #[test]
    fn letter_md_inline_code_and_mark() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("`prove`"),
            "letter.md must keep the inline code span"
        );
        assert!(
            text.contains("==three hashes=="),
            "letter.md must keep the ==highlight== span"
        );
        let doc = read(&md).expect("docagent-md read letter.md");
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
                r.style.code
                    && r.style.font.as_deref() == Some(CODE_FONT)
                    && r.style.latin_font.as_deref() == Some(CODE_FONT)
                    && matches!(&r.content, RunContent::Text(t) if t == "prove")
            }),
            "letter.md IR must keep `prove` as Consolas inline code, not article serif"
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
        let table = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Table(t) => Some(t),
                _ => None,
            })
            .expect("letter table");
        assert_eq!(table.rows[0].cells[0].padding.left, CELL_PAD_X_HU);
        let h1 = match &doc.sections[0].body[0] {
            Block::Paragraph(p) => p,
            _ => panic!("h1"),
        };
        assert_eq!(h1.runs[0].style.size, H1_SIZE_HU);
        let items: Vec<_> = doc.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) if p.numbering.is_some() => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(items[0].indent_left, LIST_INDENT_HU);
        let quote = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) if p.quote => Some(p),
                _ => None,
            })
            .expect("quote");
        assert_eq!(quote.space_before, QUOTE_BEFORE_HU);
        assert!(
            doc.sections[0]
                .body
                .iter()
                .any(|b| matches!(b, Block::Break(BreakKind::Thematic))),
            "thematic break must not regress"
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(back.contains("`prove`"), "{back}");
        assert!(back.contains("==three hashes=="), "{back}");
        assert!(
            back.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "{back}"
        );
    }

    #[test]
    fn letter_md_strike_and_underline() {
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
        let doc = read(&md).expect("docagent-md read letter.md");
        assert!(
            doc.sections[0].body.iter().any(|b| match b {
                Block::Paragraph(p) => p.runs.iter().any(|r| {
                    r.style.strike && matches!(&r.content, RunContent::Text(t) if t == "guess")
                }),
                _ => false,
            }),
            "letter.md IR must keep ~~guess~~ as strike"
        );
        assert!(
            doc.sections[0].body.iter().any(|b| match b {
                Block::Paragraph(p) => p.runs.iter().any(|r| {
                    r.style.underline != docagent_model::Underline::None
                        && matches!(&r.content, RunContent::Text(t) if t == "09:00 local")
                }),
                _ => false,
            }),
            "letter.md IR must keep __09:00 local__ as underline"
        );
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let table = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Table(t) => Some(t),
                _ => None,
            })
            .expect("letter table");
        assert_eq!(
            table.rows[0].cells[0].shading,
            Some(TH_BG),
            "th fill must not regress"
        );
        assert_eq!(table.rows[0].cells[0].padding.left, CELL_PAD_X_HU);
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
        assert!(
            doc.sections[0].body.iter().any(|b| match b {
                Block::Paragraph(p) => p.runs.iter().any(|r| {
                    r.style.code && matches!(&r.content, RunContent::Text(t) if t == "prove")
                }),
                _ => false,
            }),
            "inline code must not regress"
        );
        assert!(
            doc.sections[0].body.iter().any(|b| match b {
                Block::Paragraph(p) => p.runs.iter().any(|r| {
                    r.style.highlight == Some(docagent_model::Color::MARK)
                        && matches!(&r.content, RunContent::Text(t) if t == "three hashes")
                }),
                _ => false,
            }),
            "mark highlight must not regress"
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(back.contains("~~guess~~"), "{back}");
        assert!(back.contains("__09:00 local__"), "{back}");
        assert!(!back.contains("~~Agents"), "{back}");
        assert!(!back.contains("__Please"), "{back}");
    }

    #[test]
    fn letter_md_super_and_sub() {
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
        let doc = read(&md).expect("docagent-md read letter.md");
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
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let table = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Table(t) => Some(t),
                _ => None,
            })
            .expect("letter table");
        assert_eq!(
            table.rows[0].cells[0].shading,
            Some(TH_BG),
            "th fill must not regress"
        );
        assert_eq!(table.rows[0].cells[0].padding.left, CELL_PAD_X_HU);
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
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(back.contains("PDF/^A^"), "{back}");
        assert!(back.contains("H~2~O"), "{back}");
        assert!(!back.contains("^the"), "{back}");
        assert!(!back.contains("~Wet"), "{back}");
        assert!(back.contains("~~guess~~"), "{back}");
    }

    #[test]
    fn letter_md_strong_and_em() {
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
        let doc = read(&md).expect("docagent-md read letter.md");
        assert!(
            doc.sections[0].body.iter().any(|b| match b {
                Block::Paragraph(p) => p.runs.iter().any(|r| {
                    r.style.bold && matches!(&r.content, RunContent::Text(t) if t == "GPU-free")
                }),
                _ => false,
            }),
            "letter.md IR must keep **GPU-free** as bold"
        );
        assert!(
            doc.sections[0].body.iter().any(|b| match b {
                Block::Paragraph(p) => p.runs.iter().any(|r| {
                    r.style.italic
                        && matches!(&r.content, RunContent::Text(t) if t == "byte-for-byte")
                }),
                _ => false,
            }),
            "letter.md IR must keep _byte-for-byte_ as italic"
        );
        assert!(
            doc.sections[0].body.iter().any(|b| match b {
                Block::Paragraph(p) => p.runs.iter().any(|r| {
                    r.style.superscript && matches!(&r.content, RunContent::Text(t) if t == "A")
                }),
                _ => false,
            }),
            "super must not regress"
        );
        assert!(
            doc.sections[0].body.iter().any(|b| match b {
                Block::Paragraph(p) => p.runs.iter().any(|r| {
                    r.style.subscript && matches!(&r.content, RunContent::Text(t) if t == "2")
                }),
                _ => false,
            }),
            "sub must not regress"
        );
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let table = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Table(t) => Some(t),
                _ => None,
            })
            .expect("letter table");
        assert_eq!(
            table.rows[0].cells[0].shading,
            Some(TH_BG),
            "th fill must not regress"
        );
        assert_eq!(table.rows[0].cells[0].padding.left, CELL_PAD_X_HU);
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
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(back.contains("**GPU-free**"), "{back}");
        assert!(back.contains("_byte-for-byte_"), "{back}");
        assert!(!back.contains("**one"), "{back}");
        assert!(back.contains("PDF/^A^"), "{back}");
        assert!(back.contains("H~2~O"), "{back}");
        assert!(back.contains("- [x] Receiving dock is clear"), "{back}");
    }

    #[test]
    fn letter_md_quote_uses_weasyprint_bar_color() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("> Replay is evidence"),
            "letter.md must keep the blockquote WeasyPrint paints with a 4px left bar"
        );
        let doc = read(&md).expect("docagent-md read letter.md");
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
            "letter.md quote text must stay #134e4a like border-left:4px solid #134e4a"
        );
        assert_ne!(
            quote.runs[0].style.color,
            Color::BLACK,
            "quote text must not stay body black"
        );
        assert_eq!(QUOTE_BAR_W_HU, 300);
        assert_eq!(QUOTE_FG, TH_FG);
        let table = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Table(t) => Some(t),
                _ => None,
            })
            .expect("letter table");
        assert_eq!(
            table.rows[0].cells[0].shading,
            Some(TH_BG),
            "th must not regress"
        );
        assert_eq!(table.rows[0].cells[0].padding.left, CELL_PAD_X_HU);
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
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
        assert!(
            doc.sections[0].body.iter().any(|b| match b {
                Block::Paragraph(p) => p.runs.iter().any(|r| {
                    r.style.bold && matches!(&r.content, RunContent::Text(t) if t == "GPU-free")
                }),
                _ => false,
            }),
            "strong must not regress"
        );
        assert!(
            doc.sections[0].body.iter().any(|b| match b {
                Block::Paragraph(p) => p.runs.iter().any(|r| {
                    r.style.italic
                        && matches!(&r.content, RunContent::Text(t) if t == "byte-for-byte")
                }),
                _ => false,
            }),
            "em must not regress"
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(back.contains("> Replay is evidence"), "{back}");
    }

    #[test]
    fn letter_md_code_uses_consolas_keep_landed() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = read(&md).expect("docagent-md read letter.md");
        let prove = doc.sections[0].body.iter().find_map(|b| match b {
            Block::Paragraph(p) => p.runs.iter().find(|r| {
                r.style.code && matches!(&r.content, RunContent::Text(t) if t == "prove")
            }),
            _ => None,
        });
        let prove = prove.expect("`prove`");
        assert_eq!(
            prove.style.font.as_deref(),
            Some(CODE_FONT),
            "letter.md `prove` must be Consolas like letter.html"
        );
        assert_eq!(prove.style.latin_font.as_deref(), Some(CODE_FONT));
        assert_ne!(prove.style.font.as_deref(), Some("serif"));
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
                .all(|r| r.style.font.as_deref() == Some(CODE_FONT)),
            "letter.md fence must stay Consolas, not generic serif"
        );
        let quote = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) if p.quote => Some(p),
                _ => None,
            })
            .expect("quote");
        assert_eq!(quote.space_before, QUOTE_BEFORE_HU);
        assert!(quote.runs.iter().any(|r| r.style.color == QUOTE_FG));
        assert_eq!(QUOTE_BAR_W_HU, 300);
        let table = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Table(t) => Some(t),
                _ => None,
            })
            .expect("letter table");
        assert_eq!(
            table.rows[0].cells[0].shading,
            Some(TH_BG),
            "th must not regress"
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
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(back.contains("`prove`"), "{back}");
        assert!(back.contains("```"), "{back}");
    }

    #[test]
    fn letter_md_article_measure_keeps_landed() {
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48);
        assert_ne!(
            ARTICLE_MAX_WIDTH_REM, 100,
            "100rem is a full-bleed column, not letter.html 48rem"
        );
        assert_eq!(PAGE_RULE, "@page{size:A4;margin:2.5rem 2.75rem}");
        assert_ne!(
            PAGE_RULE, "@page{margin:75px}",
            "WeasyPrint UA @page 75px is not letter.html"
        );
        assert_eq!(BODY_LEADING_PCT, 145, "10pt / 1.45 sizes the 48rem column");
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = read(&md).expect("docagent-md read letter.md");
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
        assert_eq!(
            body.line_spacing,
            LineSpacing::Percent(BODY_LEADING_PCT),
            "body leading must stay CSS 1.45 so the 48rem measure stays readable"
        );
        assert!(
            body.runs.iter().any(|r| {
                r.style.code
                    && r.style.font.as_deref() == Some(CODE_FONT)
                    && matches!(&r.content, RunContent::Text(t) if t == "prove")
            }),
            "letter.md `prove` must stay Consolas"
        );
        let quote = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) if p.quote => Some(p),
                _ => None,
            })
            .expect("quote");
        assert_eq!(quote.space_before, QUOTE_BEFORE_HU);
        assert!(quote.runs.iter().any(|r| r.style.color == QUOTE_FG));
        assert_eq!(QUOTE_BAR_W_HU, 300);
        let table = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Table(t) => Some(t),
                _ => None,
            })
            .expect("letter table");
        assert_eq!(
            table.rows[0].cells[0].shading,
            Some(TH_BG),
            "th must not regress"
        );
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(back.contains("`prove`"), "{back}");
        assert!(back.contains("> Replay is evidence"), "{back}");
        assert!(back.contains("| **Hop** |"), "{back}");
        assert!(back.contains("![Harbor mark]"), "{back}");
    }

    #[test]
    fn letter_md_print_page_margins_keep_landed() {
        assert_eq!(PAGE_RULE, "@page{size:A4;margin:2.5rem 2.75rem}");
        assert!(
            PAGE_RULE.contains("2.5rem 2.75rem"),
            "print margins must match letter.html article padding"
        );
        assert_ne!(
            PAGE_RULE, "@page{margin:75px}",
            "WeasyPrint UA @page margin 75px is not letter.html"
        );
        assert_ne!(PAGE_RULE, "@page{margin:0}");
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48, "48rem measure must not regress");
        assert_eq!(CODE_FONT, "Consolas", "Consolas must not regress");
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * docagent_model::HU_PER_POINT);
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
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
        let quote = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) if p.quote => Some(p),
                _ => None,
            })
            .expect("quote");
        assert!(quote.runs.iter().any(|r| r.style.color == QUOTE_FG));
        let table = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Table(t) => Some(t),
                _ => None,
            })
            .expect("letter table");
        assert_eq!(
            table.rows[0].cells[0].shading,
            Some(TH_BG),
            "th must not regress"
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(back.contains("![Harbor mark]"), "{back}");
        assert!(back.contains("`prove`"), "{back}");
    }

    #[test]
    fn body_text_uses_weasyprint_teal() {
        let doc = read(b"The shipment left Busan.\n").unwrap();
        let Block::Paragraph(p) = &doc.sections[0].body[0] else {
            panic!("para");
        };
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
        assert!(
            p.runs.iter().any(|r| {
                r.style.color == BODY_FG
                    && matches!(&r.content, RunContent::Text(t) if t.contains("shipment"))
            }),
            "body text must stay letter CSS #134e4a, not UA black: {:?}",
            p.runs.iter().map(|r| r.style.color).collect::<Vec<_>>()
        );
        assert!(
            p.runs
                .iter()
                .filter(|r| matches!(r.content, RunContent::Text(_)))
                .all(|r| r.style.color != Color::BLACK),
            "body text must not keep Color::BLACK: {:?}",
            p.runs.iter().map(|r| r.style.color).collect::<Vec<_>>()
        );
        assert_eq!(p.line_spacing, LineSpacing::Percent(BODY_LEADING_PCT));
        assert_eq!(PAGE_RULE, "@page{size:A4;margin:2.5rem 2.75rem}");
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48);
        assert_eq!(CODE_FONT, "Consolas");
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * docagent_model::HU_PER_POINT);
    }

    #[test]
    fn letter_md_body_color_keeps_landed() {
        assert_eq!(
            BODY_FG,
            Color {
                r: 19,
                g: 78,
                b: 74,
                a: 255
            }
        );
        assert_eq!(PAGE_RULE, "@page{size:A4;margin:2.5rem 2.75rem}");
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48, "48rem measure must not regress");
        assert_eq!(CODE_FONT, "Consolas", "Consolas must not regress");
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * docagent_model::HU_PER_POINT);
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = read(&md).expect("docagent-md read letter.md");
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
            body.runs
                .iter()
                .filter(|r| matches!(r.content, RunContent::Text(_)) && !r.style.code)
                .all(|r| r.style.color == BODY_FG),
            "letter.md body runs must stay #134e4a: {:?}",
            body.runs
                .iter()
                .map(|r| (r.style.color, r.style.code, r.display_text().to_string()))
                .collect::<Vec<_>>()
        );
        assert!(
            body.runs.iter().any(|r| {
                r.style.code
                    && r.style.font.as_deref() == Some(CODE_FONT)
                    && r.style.color == BODY_FG
                    && matches!(&r.content, RunContent::Text(t) if t == "prove")
            }),
            "letter.md `prove` must stay Consolas teal"
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
        let table = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Table(t) => Some(t),
                _ => None,
            })
            .expect("letter table");
        assert_eq!(
            table.rows[0].cells[0].shading,
            Some(TH_BG),
            "th must not regress"
        );
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(back.contains("`prove`"), "{back}");
        assert!(back.contains("> Replay is evidence"), "{back}");
        assert!(back.contains("![Harbor mark]"), "{back}");
    }

    #[test]
    fn letter_md_link_hover_keeps_landed() {
        assert_eq!(LINK_RULE, "a{color:#0d9488;text-decoration:underline}");
        assert_eq!(LINK_HOVER_RULE, "a:hover{color:#134e4a}");
        assert_ne!(
            LINK_HOVER_RULE, "a:hover{color:blue}",
            "UA blue hover is not WeasyPrint letter input"
        );
        assert_eq!(
            BODY_FG,
            Color {
                r: 19,
                g: 78,
                b: 74,
                a: 255
            }
        );
        assert_eq!(PAGE_RULE, "@page{size:A4;margin:2.5rem 2.75rem}");
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48, "48rem measure must not regress");
        assert_eq!(CODE_FONT, "Consolas", "Consolas must not regress");
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * docagent_model::HU_PER_POINT);
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "letter.md must keep the DocAgent hyperlink HTML paints with a:hover"
        );
        let doc = read(&md).expect("docagent-md read letter.md");
        let link = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) => p.runs.iter().find(|r| {
                    matches!(
                        &r.content,
                        RunContent::Inline(InlineObject::Hyperlink { target, display })
                            if target == "https://github.com/kevin9327/docagent"
                                && display == "DocAgent"
                    )
                }),
                _ => None,
            })
            .expect("letter hyperlink");
        assert_eq!(link.style.underline, docagent_model::Underline::Single);
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
            "letter.md body text must stay letter CSS #134e4a"
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
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(
            back.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "{back}"
        );
        assert!(back.contains("`prove`"), "{back}");
        assert!(back.contains("![Harbor mark]"), "{back}");
    }

    #[test]
    fn letter_md_link_visited_focus_keeps_landed() {
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
        assert_eq!(
            BODY_FG,
            Color {
                r: 19,
                g: 78,
                b: 74,
                a: 255
            }
        );
        assert_eq!(PAGE_RULE, "@page{size:A4;margin:2.5rem 2.75rem}");
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48, "48rem measure must not regress");
        assert_eq!(CODE_FONT, "Consolas", "Consolas must not regress");
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * docagent_model::HU_PER_POINT);
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "letter.md must keep the DocAgent hyperlink HTML paints with visited/focus"
        );
        let doc = read(&md).expect("docagent-md read letter.md");
        let link = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) => p.runs.iter().find(|r| {
                    matches!(
                        &r.content,
                        RunContent::Inline(InlineObject::Hyperlink { target, display })
                            if target == "https://github.com/kevin9327/docagent"
                                && display == "DocAgent"
                    )
                }),
                _ => None,
            })
            .expect("letter hyperlink");
        assert_eq!(link.style.underline, docagent_model::Underline::Single);
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
            "letter.md body text must stay letter CSS #134e4a"
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
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(
            back.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "{back}"
        );
        assert!(back.contains("`prove`"), "{back}");
        assert!(back.contains("![Harbor mark]"), "{back}");
    }

    #[test]
    fn letter_md_selection_color_keeps_landed() {
        assert_eq!(
            SELECTION_RULE,
            "::selection{background:#ccfbf1;color:#134e4a}::-moz-selection{background:#ccfbf1;color:#134e4a}"
        );
        assert!(
            SELECTION_RULE.contains("::selection{background:#ccfbf1;color:#134e4a}")
                && SELECTION_RULE.contains("::-moz-selection{background:#ccfbf1;color:#134e4a}"),
            "selection must stay mint chip + body teal, not UA Highlight"
        );
        assert_ne!(
            SELECTION_RULE, "::selection{background:Highlight;color:HighlightText}",
            "UA Highlight/HighlightText is system blue, not letter.html"
        );
        assert_ne!(SELECTION_RULE, "::selection{background:blue}");
        assert_eq!(LINK_RULE, "a{color:#0d9488;text-decoration:underline}");
        assert_eq!(LINK_VISITED_RULE, "a:visited{color:#0d9488}");
        assert_eq!(LINK_HOVER_RULE, "a:hover{color:#134e4a}");
        assert_eq!(LINK_FOCUS_RULE, "a:focus{color:#134e4a}");
        assert_eq!(
            BODY_FG,
            Color {
                r: 19,
                g: 78,
                b: 74,
                a: 255
            }
        );
        assert_eq!(
            TH_BG,
            Color {
                r: 204,
                g: 251,
                b: 241,
                a: 255
            }
        );
        assert_eq!(PAGE_RULE, "@page{size:A4;margin:2.5rem 2.75rem}");
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48, "48rem measure must not regress");
        assert_eq!(CODE_FONT, "Consolas", "Consolas must not regress");
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * docagent_model::HU_PER_POINT);
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = read(&md).expect("docagent-md read letter.md");
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
            "letter.md body text must stay letter CSS #134e4a"
        );
        assert!(
            body.runs.iter().any(|r| {
                r.style.code
                    && r.style.font.as_deref() == Some(CODE_FONT)
                    && matches!(&r.content, RunContent::Text(t) if t == "prove")
            }),
            "letter.md `prove` must stay Consolas"
        );
        let table = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Table(t) => Some(t),
                _ => None,
            })
            .expect("letter table");
        assert_eq!(
            table.rows[0].cells[0].shading,
            Some(TH_BG),
            "th mint that selection matches must not regress"
        );
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(back.contains("`prove`"), "{back}");
        assert!(back.contains("![Harbor mark]"), "{back}");
        assert!(
            back.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "{back}"
        );
    }

    #[test]
    fn letter_md_heading_rule_keeps_landed() {
        assert_eq!(
            HEADING_RULE,
            "h1{font-size:1.75rem;line-height:1.2;margin:0 0 .6rem;letter-spacing:-.02em;padding-bottom:.4rem;border-bottom:3px solid #134e4a}h2{font-size:1.15rem;line-height:1.2;margin:1.1rem 0 .45rem;color:#134e4a;padding-bottom:.2rem;border-bottom:2px solid #0d9488}"
        );
        assert!(
            HEADING_RULE.contains("padding-bottom:.4rem;border-bottom:3px solid #134e4a")
                && HEADING_RULE.contains("padding-bottom:.2rem;border-bottom:2px solid #0d9488")
                && HEADING_RULE.contains("letter-spacing:-.02em"),
            "h1/h2 must keep letter.html teal bottom rules, not UA unruled titles"
        );
        assert_ne!(
            HEADING_RULE, "h1{font-size:2em;margin:.67em 0}h2{font-size:1.5em;margin:.75em 0}",
            "WeasyPrint UA h1/h2 has no teal bottom rule"
        );
        assert_ne!(HEADING_RULE, "h1{font-size:2em;margin:.67em 0}");
        assert_eq!(H1_SIZE_HU, 2100);
        assert_eq!(H2_SIZE_HU, 1380);
        assert_eq!(HEADING_LEADING_PCT, 120);
        assert_eq!(SELECTION_RULE, "::selection{background:#ccfbf1;color:#134e4a}::-moz-selection{background:#ccfbf1;color:#134e4a}");
        assert_eq!(LINK_RULE, "a{color:#0d9488;text-decoration:underline}");
        assert_eq!(LINK_VISITED_RULE, "a:visited{color:#0d9488}");
        assert_eq!(LINK_HOVER_RULE, "a:hover{color:#134e4a}");
        assert_eq!(LINK_FOCUS_RULE, "a:focus{color:#134e4a}");
        assert_eq!(
            BODY_FG,
            Color {
                r: 19,
                g: 78,
                b: 74,
                a: 255
            }
        );
        assert_eq!(
            TH_BG,
            Color {
                r: 204,
                g: 251,
                b: 241,
                a: 255
            }
        );
        assert_eq!(PAGE_RULE, "@page{size:A4;margin:2.5rem 2.75rem}");
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48, "48rem measure must not regress");
        assert_eq!(CODE_FONT, "Consolas", "Consolas must not regress");
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * docagent_model::HU_PER_POINT);
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = read(&md).expect("docagent-md read letter.md");
        let h1 = match &doc.sections[0].body[0] {
            Block::Paragraph(p) => p,
            _ => panic!("h1"),
        };
        let h2 = match &doc.sections[0].body[1] {
            Block::Paragraph(p) => p,
            _ => panic!("h2"),
        };
        assert_eq!(h1.plain_text(), "Northwind Freight");
        assert_eq!(h1.outline_level, Some(1));
        assert_eq!(h1.runs[0].style.size, H1_SIZE_HU);
        assert!(h1.runs[0].style.bold);
        assert_eq!(h1.line_spacing, LineSpacing::Percent(HEADING_LEADING_PCT));
        assert_eq!(h2.plain_text(), "Delivery confirmation — order 48291");
        assert_eq!(h2.outline_level, Some(2));
        assert_eq!(h2.runs[0].style.size, H2_SIZE_HU);
        assert!(h2.runs[0].style.bold);
        assert_eq!(h2.line_spacing, LineSpacing::Percent(HEADING_LEADING_PCT));
        let table = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Table(t) => Some(t),
                _ => None,
            })
            .expect("letter table");
        assert_eq!(
            table.rows[0].cells[0].shading,
            Some(TH_BG),
            "th fill must not regress"
        );
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(back.contains("# Northwind Freight"), "{back}");
        assert!(back.contains("## Delivery confirmation"), "{back}");
        assert!(back.contains("`prove`"), "{back}");
        assert!(
            back.contains("![Harbor mark](docs/assets/mark.png)"),
            "letter.md image path must round-trip: {back}"
        );
        assert!(
            !back.contains("![Harbor mark](data:image/png;base64,"),
            "letter.md path must not rewrite to a data URI: {back}"
        );
        assert!(
            back.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "{back}"
        );
    }

    #[test]
    fn letter_md_image_path_roundtrips() {
        assert_eq!(LETTER_IMAGE_PATH, "docs/assets/mark.png");
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * docagent_model::HU_PER_POINT);
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("![Harbor mark](docs/assets/mark.png)"),
            "letter.md must keep the relative harbor-mark path"
        );
        let doc = read(&md).expect("docagent-md read letter.md");
        let png = fixture_png();
        let img = first_image(&doc);
        assert_eq!(img.bytes, png);
        assert_eq!(img.alt_text.as_deref(), Some("Harbor mark"));
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(
            back.contains("![Harbor mark](docs/assets/mark.png)"),
            "letter.md → IR → markdown must keep docs/assets/mark.png: {back}"
        );
        assert!(
            !back.contains("data:image/png;base64,"),
            "letter.md path round-trip must not emit a data URI: {back}"
        );
        let again = read(back.as_bytes()).expect("round-trip md");
        assert_eq!(first_image(&again).bytes, png);
        assert_eq!(
            (first_image(&again).width, first_image(&again).height),
            (MIN_IMAGE_EDGE_HU, MIN_IMAGE_EDGE_HU),
            "path round-trip must keep the PDF 24pt floor"
        );
        let twice = String::from_utf8(write(&again).unwrap()).unwrap();
        assert!(
            twice.contains("![Harbor mark](docs/assets/mark.png)"),
            "second write must keep the relative path: {twice}"
        );
    }

    #[test]
    fn images_unknown_bytes_write_data_uri() {
        let img = ImageData {
            bytes: vec![0xFF, 0xD8, 0xFF, 0xD9],
            mime: "image/jpeg".into(),
            width: MIN_IMAGE_EDGE_HU,
            height: MIN_IMAGE_EDGE_HU,
            alt_text: Some("j".into()),
            wrap: WrapMode::Inline,
        };
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![Run {
            style: docagent_model::CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(img.clone())),
        }];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let text = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(
            text.contains("![j](data:image/jpeg;base64,"),
            "bytes that are not the letter fixture must still write a data URI: {text}"
        );
        assert!(
            !text.contains("docs/assets/mark.png"),
            "unknown bytes must not steal the letter path: {text}"
        );
        let again_doc = read(text.as_bytes()).unwrap();
        let again = first_image(&again_doc);
        assert_eq!(again.bytes, img.bytes);
        assert_eq!(again.mime, "image/jpeg");
    }

    #[test]
    fn letter_md_abbr_dotted_teal_keeps_landed() {
        assert_eq!(
            ABBR_RULE,
            "abbr[title],acronym[title]{text-decoration:underline dotted #134e4a;text-underline-offset:.15em;cursor:help}"
        );
        assert!(
            ABBR_RULE.contains("text-decoration:underline dotted #134e4a")
                && ABBR_RULE.contains("text-underline-offset:.15em")
                && ABBR_RULE.contains("cursor:help"),
            "abbr must stay dotted body teal, not UA uncolored CanvasText"
        );
        assert_ne!(
            ABBR_RULE, "abbr[title],acronym[title]{text-decoration:dotted underline}",
            "WeasyPrint UA abbr dotted underline is uncolored, not letter teal"
        );
        assert_eq!(
            HEADING_RULE,
            "h1{font-size:1.75rem;line-height:1.2;margin:0 0 .6rem;letter-spacing:-.02em;padding-bottom:.4rem;border-bottom:3px solid #134e4a}h2{font-size:1.15rem;line-height:1.2;margin:1.1rem 0 .45rem;color:#134e4a;padding-bottom:.2rem;border-bottom:2px solid #0d9488}"
        );
        assert_eq!(
            SELECTION_RULE,
            "::selection{background:#ccfbf1;color:#134e4a}::-moz-selection{background:#ccfbf1;color:#134e4a}"
        );
        assert_eq!(LINK_RULE, "a{color:#0d9488;text-decoration:underline}");
        assert_eq!(LINK_VISITED_RULE, "a:visited{color:#0d9488}");
        assert_eq!(LINK_HOVER_RULE, "a:hover{color:#134e4a}");
        assert_eq!(LINK_FOCUS_RULE, "a:focus{color:#134e4a}");
        assert_eq!(PAGE_RULE, "@page{size:A4;margin:2.5rem 2.75rem}");
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48, "48rem measure must not regress");
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * docagent_model::HU_PER_POINT);
        assert_eq!(LETTER_IMAGE_PATH, "docs/assets/mark.png");
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(
            back.contains("![Harbor mark](docs/assets/mark.png)"),
            "letter.md image path must round-trip: {back}"
        );
        assert!(back.contains("`prove`"), "{back}");
        assert!(
            back.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "{back}"
        );
    }

    #[test]
    fn letter_md_details_summary_keeps_landed() {
        assert_eq!(
            DETAILS_RULE,
            "details{margin:0 0 .7rem;color:#134e4a}summary{display:block;cursor:pointer;font-weight:700;color:#134e4a;list-style:none}details[open]>summary{list-style:none}"
        );
        assert!(
            DETAILS_RULE.contains("details{margin:0 0 .7rem;color:#134e4a}")
                && DETAILS_RULE.contains("summary{display:block;cursor:pointer;font-weight:700;color:#134e4a;list-style:none}")
                && DETAILS_RULE.contains("details[open]>summary{list-style:none}"),
            "details/summary must stay block body teal, not UA list-item ▸"
        );
        assert_ne!(
            DETAILS_RULE, "summary{display:list-item}",
            "HTML UA summary list-item paints a disclosure marker, not letter block teal"
        );
        assert_eq!(
            ABBR_RULE,
            "abbr[title],acronym[title]{text-decoration:underline dotted #134e4a;text-underline-offset:.15em;cursor:help}"
        );
        assert_eq!(
            HEADING_RULE,
            "h1{font-size:1.75rem;line-height:1.2;margin:0 0 .6rem;letter-spacing:-.02em;padding-bottom:.4rem;border-bottom:3px solid #134e4a}h2{font-size:1.15rem;line-height:1.2;margin:1.1rem 0 .45rem;color:#134e4a;padding-bottom:.2rem;border-bottom:2px solid #0d9488}"
        );
        assert_eq!(
            SELECTION_RULE,
            "::selection{background:#ccfbf1;color:#134e4a}::-moz-selection{background:#ccfbf1;color:#134e4a}"
        );
        assert_eq!(LINK_RULE, "a{color:#0d9488;text-decoration:underline}");
        assert_eq!(LINK_VISITED_RULE, "a:visited{color:#0d9488}");
        assert_eq!(LINK_HOVER_RULE, "a:hover{color:#134e4a}");
        assert_eq!(LINK_FOCUS_RULE, "a:focus{color:#134e4a}");
        assert_eq!(PAGE_RULE, "@page{size:A4;margin:2.5rem 2.75rem}");
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48, "48rem measure must not regress");
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * docagent_model::HU_PER_POINT);
        assert_eq!(LETTER_IMAGE_PATH, "docs/assets/mark.png");
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(
            back.contains("![Harbor mark](docs/assets/mark.png)"),
            "letter.md image path must round-trip: {back}"
        );
        assert!(back.contains("`prove`"), "{back}");
        assert!(
            back.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "{back}"
        );
    }

    #[test]
    fn letter_md_tbody_row_group_keeps_landed() {
        assert_eq!(TBODY_RULE, "tbody{display:table-row-group}");
        assert!(
            TBODY_RULE.contains("tbody{display:table-row-group}"),
            "tbody must stay table-row-group, not a display:block table reset"
        );
        assert_ne!(
            TBODY_RULE, "tbody{display:block}",
            "display:block tbody stacks hop body rows, not letter row-group"
        );
        assert_eq!(
            DETAILS_RULE,
            "details{margin:0 0 .7rem;color:#134e4a}summary{display:block;cursor:pointer;font-weight:700;color:#134e4a;list-style:none}details[open]>summary{list-style:none}"
        );
        assert_eq!(
            ABBR_RULE,
            "abbr[title],acronym[title]{text-decoration:underline dotted #134e4a;text-underline-offset:.15em;cursor:help}"
        );
        assert_eq!(
            HEADING_RULE,
            "h1{font-size:1.75rem;line-height:1.2;margin:0 0 .6rem;letter-spacing:-.02em;padding-bottom:.4rem;border-bottom:3px solid #134e4a}h2{font-size:1.15rem;line-height:1.2;margin:1.1rem 0 .45rem;color:#134e4a;padding-bottom:.2rem;border-bottom:2px solid #0d9488}"
        );
        assert_eq!(
            SELECTION_RULE,
            "::selection{background:#ccfbf1;color:#134e4a}::-moz-selection{background:#ccfbf1;color:#134e4a}"
        );
        assert_eq!(LINK_RULE, "a{color:#0d9488;text-decoration:underline}");
        assert_eq!(LINK_VISITED_RULE, "a:visited{color:#0d9488}");
        assert_eq!(LINK_HOVER_RULE, "a:hover{color:#134e4a}");
        assert_eq!(LINK_FOCUS_RULE, "a:focus{color:#134e4a}");
        assert_eq!(PAGE_RULE, "@page{size:A4;margin:2.5rem 2.75rem}");
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48, "48rem measure must not regress");
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * docagent_model::HU_PER_POINT);
        assert_eq!(LETTER_IMAGE_PATH, "docs/assets/mark.png");
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let t = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Table(table) => Some(table),
                _ => None,
            })
            .expect("letter table");
        assert!(t.rows[0].header, "letter hop row must be a header");
        assert!(!t.rows[1].header, "letter hop body must stay a body row");
        assert_eq!(t.header_row_count, 1);
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(
            back.contains("![Harbor mark](docs/assets/mark.png)"),
            "letter.md image path must round-trip: {back}"
        );
        assert!(back.contains("| **Hop** |") && back.contains("| Input | letter.md |"), "{back}");
        assert!(back.contains("`prove`"), "{back}");
        assert!(
            back.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "{back}"
        );
    }

    #[test]
    fn letter_md_table_collapse_keeps_landed() {
        assert_eq!(
            TABLE_RULE,
            "table{border-collapse:collapse;margin:1rem 0;width:100%}"
        );
        assert!(
            TABLE_RULE.contains("border-collapse:collapse")
                && TABLE_RULE.contains("margin:1rem 0")
                && TABLE_RULE.contains("width:100%"),
            "table must stay collapsed full-width, not UA separate 2px spacing"
        );
        assert_ne!(
            TABLE_RULE, "table{border-collapse:separate}",
            "WeasyPrint UA separate collapse punches sparse hop-cell gaps"
        );
        assert_ne!(
            TABLE_RULE, "table{border-collapse:separate;border-spacing:2px}",
            "UA border-spacing:2px is not letter.html hop table"
        );
        assert_eq!(TBODY_RULE, "tbody{display:table-row-group}");
        assert_eq!(
            DETAILS_RULE,
            "details{margin:0 0 .7rem;color:#134e4a}summary{display:block;cursor:pointer;font-weight:700;color:#134e4a;list-style:none}details[open]>summary{list-style:none}"
        );
        assert_eq!(
            ABBR_RULE,
            "abbr[title],acronym[title]{text-decoration:underline dotted #134e4a;text-underline-offset:.15em;cursor:help}"
        );
        assert_eq!(
            HEADING_RULE,
            "h1{font-size:1.75rem;line-height:1.2;margin:0 0 .6rem;letter-spacing:-.02em;padding-bottom:.4rem;border-bottom:3px solid #134e4a}h2{font-size:1.15rem;line-height:1.2;margin:1.1rem 0 .45rem;color:#134e4a;padding-bottom:.2rem;border-bottom:2px solid #0d9488}"
        );
        assert_eq!(
            SELECTION_RULE,
            "::selection{background:#ccfbf1;color:#134e4a}::-moz-selection{background:#ccfbf1;color:#134e4a}"
        );
        assert_eq!(LINK_RULE, "a{color:#0d9488;text-decoration:underline}");
        assert_eq!(LINK_VISITED_RULE, "a:visited{color:#0d9488}");
        assert_eq!(LINK_HOVER_RULE, "a:hover{color:#134e4a}");
        assert_eq!(LINK_FOCUS_RULE, "a:focus{color:#134e4a}");
        assert_eq!(PAGE_RULE, "@page{size:A4;margin:2.5rem 2.75rem}");
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48, "48rem measure must not regress");
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * docagent_model::HU_PER_POINT);
        assert_eq!(LETTER_IMAGE_PATH, "docs/assets/mark.png");
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let t = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Table(table) => Some(table),
                _ => None,
            })
            .expect("letter table");
        assert!(t.rows[0].header, "letter hop row must be a header");
        assert!(!t.rows[1].header, "letter hop body must stay a body row");
        assert_eq!(t.header_row_count, 1);
        assert_eq!(
            t.rows[0].cells[0].shading,
            Some(TH_BG),
            "th fill must not regress"
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(
            back.contains("![Harbor mark](docs/assets/mark.png)"),
            "letter.md image path must round-trip: {back}"
        );
        assert!(
            back.contains("| **Hop** |") && back.contains("| Input | letter.md |"),
            "{back}"
        );
        assert!(back.contains("`prove`"), "{back}");
        assert!(
            back.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "{back}"
        );
    }

    #[test]
    fn letter_md_list_indent_keeps_landed() {
        assert_eq!(LIST_RULE, "ul,ol{margin:0 0 1rem;padding-left:1.25rem}");
        assert!(
            LIST_RULE.contains("padding-left:1.25rem")
                && LIST_RULE.contains("margin:0 0 1rem")
                && LIST_RULE.contains("ul,ol{"),
            "list indent must stay letter 1.25rem, not UA 40px"
        );
        assert_ne!(
            LIST_RULE, "ul,ol{padding-left:40px}",
            "WeasyPrint UA 40px list gutter punches the 48rem column"
        );
        assert_ne!(
            LIST_RULE, "ul,ol{margin:1em 0;padding-left:40px}",
            "UA 1em/40px list box is not letter.html 1.25rem indent"
        );
        assert_eq!(LIST_INDENT_HU, 1500, "1.25rem at 16px rem / 96dpi is 1500 HU");
        assert_ne!(
            LIST_INDENT_HU, 3000,
            "UA 40px at 96dpi is 3000 HU, not letter 1.25rem"
        );
        assert_eq!(
            TABLE_RULE,
            "table{border-collapse:collapse;margin:1rem 0;width:100%}"
        );
        assert_eq!(TBODY_RULE, "tbody{display:table-row-group}");
        assert_eq!(
            DETAILS_RULE,
            "details{margin:0 0 .7rem;color:#134e4a}summary{display:block;cursor:pointer;font-weight:700;color:#134e4a;list-style:none}details[open]>summary{list-style:none}"
        );
        assert_eq!(
            ABBR_RULE,
            "abbr[title],acronym[title]{text-decoration:underline dotted #134e4a;text-underline-offset:.15em;cursor:help}"
        );
        assert_eq!(
            HEADING_RULE,
            "h1{font-size:1.75rem;line-height:1.2;margin:0 0 .6rem;letter-spacing:-.02em;padding-bottom:.4rem;border-bottom:3px solid #134e4a}h2{font-size:1.15rem;line-height:1.2;margin:1.1rem 0 .45rem;color:#134e4a;padding-bottom:.2rem;border-bottom:2px solid #0d9488}"
        );
        assert_eq!(
            SELECTION_RULE,
            "::selection{background:#ccfbf1;color:#134e4a}::-moz-selection{background:#ccfbf1;color:#134e4a}"
        );
        assert_eq!(LINK_RULE, "a{color:#0d9488;text-decoration:underline}");
        assert_eq!(LINK_VISITED_RULE, "a:visited{color:#0d9488}");
        assert_eq!(LINK_HOVER_RULE, "a:hover{color:#134e4a}");
        assert_eq!(LINK_FOCUS_RULE, "a:focus{color:#134e4a}");
        assert_eq!(PAGE_RULE, "@page{size:A4;margin:2.5rem 2.75rem}");
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48, "48rem measure must not regress");
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * docagent_model::HU_PER_POINT);
        assert_eq!(LETTER_IMAGE_PATH, "docs/assets/mark.png");
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let doc = read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let top: Vec<_> = doc.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p)
                    if p.numbering.as_ref().is_some_and(|n| n.level == 0) =>
                {
                    Some(p.indent_left)
                }
                _ => None,
            })
            .collect();
        assert!(
            !top.is_empty() && top.iter().all(|i| *i == LIST_INDENT_HU),
            "letter.md top-level lists must keep 1.25rem indent: {top:?}"
        );
        let nested: Vec<_> = doc.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p)
                    if p.numbering.as_ref().is_some_and(|n| n.level == 1) =>
                {
                    Some(p.indent_left)
                }
                _ => None,
            })
            .collect();
        assert!(
            !nested.is_empty() && nested.iter().all(|i| *i == LIST_INDENT_HU * 2),
            "letter.md nested lists must keep 2.5rem indent: {nested:?}"
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(
            back.contains("![Harbor mark](docs/assets/mark.png)"),
            "letter.md image path must round-trip: {back}"
        );
        assert!(
            back.contains("1. Hash the input") && back.contains("- [x] Receiving dock is clear"),
            "{back}"
        );
        assert!(back.contains("`prove`"), "{back}");
        assert!(
            back.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "{back}"
        );
    }

    #[test]
    fn letter_md_code_span_pad_keeps_landed() {
        assert_eq!(
            CODE_RULE,
            "code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"
        );
        assert!(
            CODE_RULE.contains("padding:.1em .35em")
                && CODE_RULE.contains("background:#ccfbf1")
                && CODE_RULE.contains("font-size:.92em")
                && CODE_RULE.contains("border-radius:3px")
                && CODE_RULE.contains("font-family:ui-monospace,Consolas,monospace"),
            "code span must keep letter .1em .35em mint pad, not UA unpadded monospace"
        );
        assert_ne!(
            CODE_RULE, "code{font-family:monospace}",
            "WeasyPrint UA code is bare monospace, not letter.html prove chip"
        );
        assert_ne!(
            CODE_RULE, "code{padding:0}",
            "zero code-span pad jams `prove` into body type"
        );
        assert_eq!(LIST_RULE, "ul,ol{margin:0 0 1rem;padding-left:1.25rem}");
        assert_eq!(
            TABLE_RULE,
            "table{border-collapse:collapse;margin:1rem 0;width:100%}"
        );
        assert_eq!(TBODY_RULE, "tbody{display:table-row-group}");
        assert_eq!(
            DETAILS_RULE,
            "details{margin:0 0 .7rem;color:#134e4a}summary{display:block;cursor:pointer;font-weight:700;color:#134e4a;list-style:none}details[open]>summary{list-style:none}"
        );
        assert_eq!(
            ABBR_RULE,
            "abbr[title],acronym[title]{text-decoration:underline dotted #134e4a;text-underline-offset:.15em;cursor:help}"
        );
        assert_eq!(
            HEADING_RULE,
            "h1{font-size:1.75rem;line-height:1.2;margin:0 0 .6rem;letter-spacing:-.02em;padding-bottom:.4rem;border-bottom:3px solid #134e4a}h2{font-size:1.15rem;line-height:1.2;margin:1.1rem 0 .45rem;color:#134e4a;padding-bottom:.2rem;border-bottom:2px solid #0d9488}"
        );
        assert_eq!(
            SELECTION_RULE,
            "::selection{background:#ccfbf1;color:#134e4a}::-moz-selection{background:#ccfbf1;color:#134e4a}"
        );
        assert_eq!(LINK_RULE, "a{color:#0d9488;text-decoration:underline}");
        assert_eq!(LINK_VISITED_RULE, "a:visited{color:#0d9488}");
        assert_eq!(LINK_HOVER_RULE, "a:hover{color:#134e4a}");
        assert_eq!(LINK_FOCUS_RULE, "a:focus{color:#134e4a}");
        assert_eq!(PAGE_RULE, "@page{size:A4;margin:2.5rem 2.75rem}");
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48, "48rem measure must not regress");
        assert_eq!(CODE_FONT, "Consolas", "Consolas must not regress");
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * docagent_model::HU_PER_POINT);
        assert_eq!(LETTER_IMAGE_PATH, "docs/assets/mark.png");
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("`prove`"),
            "letter.md must keep the inline code span WeasyPrint paints as a padded chip"
        );
        let doc = read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
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
            .expect("prove body");
        assert!(
            body.runs.iter().any(|r| {
                r.style.code
                    && r.style.font.as_deref() == Some(CODE_FONT)
                    && r.style.latin_font.as_deref() == Some(CODE_FONT)
                    && matches!(&r.content, RunContent::Text(t) if t == "prove")
            }),
            "letter.md `prove` must stay Consolas under the .1em .35em chip"
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(
            back.contains("![Harbor mark](docs/assets/mark.png)"),
            "letter.md image path must round-trip: {back}"
        );
        assert!(back.contains("`prove`"), "{back}");
        assert!(
            back.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "{back}"
        );
        assert!(
            !back.contains("code{padding:0}") && !back.contains("code{font-family:monospace}"),
            "markdown write must not emit UA unpadded code: {back}"
        );
    }

    #[test]
    fn letter_md_pre_pad_keeps_landed() {
        assert_eq!(
            PRE_RULE,
            "pre{background:#ccfbf1;padding:.75rem 1rem;margin:.6rem 0 1rem;border-radius:4px;overflow:auto;font-family:ui-monospace,Consolas,monospace}"
        );
        assert!(
            PRE_RULE.contains("padding:.75rem 1rem")
                && PRE_RULE.contains("background:#ccfbf1")
                && PRE_RULE.contains("margin:.6rem 0 1rem")
                && PRE_RULE.contains("border-radius:4px")
                && PRE_RULE.contains("font-family:ui-monospace,Consolas,monospace"),
            "pre must keep letter .75rem 1rem mint pad, not UA unpadded monospace"
        );
        assert_ne!(
            PRE_RULE, "pre{font-family:monospace}",
            "WeasyPrint UA pre is bare monospace, not letter.html prove fence"
        );
        assert_ne!(
            PRE_RULE, "pre{padding:0}",
            "zero pre pad jams the prove fence into the mint box edge"
        );
        assert_ne!(
            PRE_RULE, "pre{margin:1em 0}",
            "WeasyPrint UA 1em pre margin is sparse, not letter .75rem 1rem pad"
        );
        assert_eq!(
            CODE_RULE,
            "code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"
        );
        assert_eq!(LIST_RULE, "ul,ol{margin:0 0 1rem;padding-left:1.25rem}");
        assert_eq!(
            TABLE_RULE,
            "table{border-collapse:collapse;margin:1rem 0;width:100%}"
        );
        assert_eq!(TBODY_RULE, "tbody{display:table-row-group}");
        assert_eq!(
            DETAILS_RULE,
            "details{margin:0 0 .7rem;color:#134e4a}summary{display:block;cursor:pointer;font-weight:700;color:#134e4a;list-style:none}details[open]>summary{list-style:none}"
        );
        assert_eq!(
            ABBR_RULE,
            "abbr[title],acronym[title]{text-decoration:underline dotted #134e4a;text-underline-offset:.15em;cursor:help}"
        );
        assert_eq!(
            HEADING_RULE,
            "h1{font-size:1.75rem;line-height:1.2;margin:0 0 .6rem;letter-spacing:-.02em;padding-bottom:.4rem;border-bottom:3px solid #134e4a}h2{font-size:1.15rem;line-height:1.2;margin:1.1rem 0 .45rem;color:#134e4a;padding-bottom:.2rem;border-bottom:2px solid #0d9488}"
        );
        assert_eq!(
            SELECTION_RULE,
            "::selection{background:#ccfbf1;color:#134e4a}::-moz-selection{background:#ccfbf1;color:#134e4a}"
        );
        assert_eq!(LINK_RULE, "a{color:#0d9488;text-decoration:underline}");
        assert_eq!(LINK_VISITED_RULE, "a:visited{color:#0d9488}");
        assert_eq!(LINK_HOVER_RULE, "a:hover{color:#134e4a}");
        assert_eq!(LINK_FOCUS_RULE, "a:focus{color:#134e4a}");
        assert_eq!(PAGE_RULE, "@page{size:A4;margin:2.5rem 2.75rem}");
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48, "48rem measure must not regress");
        assert_eq!(CODE_FONT, "Consolas", "Consolas must not regress");
        assert_eq!(CODE_BEFORE_HU, 720, "pre margin-top .6rem must not regress");
        assert_eq!(CODE_AFTER_HU, 1200, "pre margin-bottom 1rem must not regress");
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * docagent_model::HU_PER_POINT);
        assert_eq!(LETTER_IMAGE_PATH, "docs/assets/mark.png");
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("docagent prove examples/letter.md"),
            "letter.md must keep the fenced prove command WeasyPrint paints in a padded pre"
        );
        let doc = read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
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
            "letter.md prove fence must stay Consolas under the .75rem 1rem pre pad"
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(
            back.contains("![Harbor mark](docs/assets/mark.png)"),
            "letter.md image path must round-trip: {back}"
        );
        assert!(
            back.contains("```") && back.contains("docagent prove examples/letter.md"),
            "markdown write must keep the prove fence: {back}"
        );
        assert!(back.contains("`prove`"), "{back}");
        assert!(
            back.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "{back}"
        );
        assert!(
            !back.contains("pre{padding:0}")
                && !back.contains("pre{margin:1em 0")
                && !back.contains("pre{font-family:monospace}")
                && !back.contains("code{padding:0}")
                && !back.contains("code{font-family:monospace}"),
            "markdown write must not emit UA unpadded pre or code: {back}"
        );
    }

    #[test]
    fn letter_md_marker_teal_keeps_landed() {
        assert_eq!(MARKER_RULE, "::marker{color:#134e4a}");
        assert!(
            MARKER_RULE.contains("::marker{color:#134e4a}"),
            "list markers must stay body teal, not UA uncolored CanvasText"
        );
        assert_ne!(
            MARKER_RULE,
            "::marker{font-variant-numeric:tabular-nums;white-space:pre;text-transform:none}",
            "WeasyPrint UA ::marker has no color, not letter.html teal"
        );
        assert_ne!(
            MARKER_RULE, "::marker{color:black}",
            "black markers punch the teal letter"
        );
        assert_ne!(
            MARKER_RULE, "::marker{color:CanvasText}",
            "CanvasText markers are UA black, not letter.html teal"
        );
        assert_eq!(
            PRE_RULE,
            "pre{background:#ccfbf1;padding:.75rem 1rem;margin:.6rem 0 1rem;border-radius:4px;overflow:auto;font-family:ui-monospace,Consolas,monospace}"
        );
        assert_eq!(
            CODE_RULE,
            "code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"
        );
        assert_eq!(LIST_RULE, "ul,ol{margin:0 0 1rem;padding-left:1.25rem}");
        assert_eq!(
            TABLE_RULE,
            "table{border-collapse:collapse;margin:1rem 0;width:100%}"
        );
        assert_eq!(TBODY_RULE, "tbody{display:table-row-group}");
        assert_eq!(
            DETAILS_RULE,
            "details{margin:0 0 .7rem;color:#134e4a}summary{display:block;cursor:pointer;font-weight:700;color:#134e4a;list-style:none}details[open]>summary{list-style:none}"
        );
        assert_eq!(
            ABBR_RULE,
            "abbr[title],acronym[title]{text-decoration:underline dotted #134e4a;text-underline-offset:.15em;cursor:help}"
        );
        assert_eq!(
            HEADING_RULE,
            "h1{font-size:1.75rem;line-height:1.2;margin:0 0 .6rem;letter-spacing:-.02em;padding-bottom:.4rem;border-bottom:3px solid #134e4a}h2{font-size:1.15rem;line-height:1.2;margin:1.1rem 0 .45rem;color:#134e4a;padding-bottom:.2rem;border-bottom:2px solid #0d9488}"
        );
        assert_eq!(
            SELECTION_RULE,
            "::selection{background:#ccfbf1;color:#134e4a}::-moz-selection{background:#ccfbf1;color:#134e4a}"
        );
        assert_eq!(LINK_RULE, "a{color:#0d9488;text-decoration:underline}");
        assert_eq!(LINK_VISITED_RULE, "a:visited{color:#0d9488}");
        assert_eq!(LINK_HOVER_RULE, "a:hover{color:#134e4a}");
        assert_eq!(LINK_FOCUS_RULE, "a:focus{color:#134e4a}");
        assert_eq!(PAGE_RULE, "@page{size:A4;margin:2.5rem 2.75rem}");
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48, "48rem measure must not regress");
        assert_eq!(CODE_FONT, "Consolas", "Consolas must not regress");
        assert_eq!(
            BODY_FG,
            Color {
                r: 19,
                g: 78,
                b: 74,
                a: 255
            }
        );
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * docagent_model::HU_PER_POINT);
        assert_eq!(LETTER_IMAGE_PATH, "docs/assets/mark.png");
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("1. Hash the input"),
            "letter.md must keep the agent-replay ol WeasyPrint paints with teal markers"
        );
        let doc = read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let decimals: Vec<_> = doc.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p)
                    if p.numbering
                        .as_ref()
                        .is_some_and(|n| n.format == Some(NumberFormat::Decimal)) =>
                {
                    Some(p)
                }
                _ => None,
            })
            .collect();
        assert!(
            decimals.iter().any(|p| p.plain_text().contains("Hash the input"))
                && decimals.iter().any(|p| p.plain_text().contains("Run Convert"))
                && decimals
                    .iter()
                    .any(|p| p.plain_text().contains("Compare the capsule")),
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
                .flat_map(|p| p.runs.iter().map(|r| (r.style.color, r.display_text().to_string())))
                .collect::<Vec<_>>()
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(
            back.contains("![Harbor mark](docs/assets/mark.png)"),
            "letter.md image path must round-trip: {back}"
        );
        assert!(
            back.contains("1. Hash the input") && back.contains("2. Run Convert"),
            "markdown write must keep the agent-replay ol: {back}"
        );
        assert!(back.contains("`prove`"), "{back}");
        assert!(
            back.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "{back}"
        );
        assert!(
            !back.contains("::marker{color:black")
                && !back.contains("::marker{color:CanvasText")
                && !back.contains("pre{padding:0}")
                && !back.contains("code{padding:0}")
                && !back.contains("code{font-family:monospace}"),
            "markdown write must not emit UA black markers or unpadded pre/code: {back}"
        );
    }

    #[test]
    fn letter_md_pre_code_zero_pad_keeps_landed() {
        assert_eq!(
            PRE_CODE_RULE,
            "pre code{background:transparent;padding:0;color:#134e4a;font-family:ui-monospace,Consolas,monospace}"
        );
        assert!(
            PRE_CODE_RULE.contains("background:transparent")
                && PRE_CODE_RULE.contains("padding:0")
                && PRE_CODE_RULE.contains("color:#134e4a")
                && PRE_CODE_RULE.contains("font-family:ui-monospace,Consolas,monospace"),
            "pre code must zero the inherited prove-chip pad, not keep mint inset"
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
            "mint fill on pre code doubles the pre box"
        );
        assert_eq!(
            PRE_RULE,
            "pre{background:#ccfbf1;padding:.75rem 1rem;margin:.6rem 0 1rem;border-radius:4px;overflow:auto;font-family:ui-monospace,Consolas,monospace}"
        );
        assert_eq!(
            CODE_RULE,
            "code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"
        );
        assert_eq!(MARKER_RULE, "::marker{color:#134e4a}");
        assert_eq!(LIST_RULE, "ul,ol{margin:0 0 1rem;padding-left:1.25rem}");
        assert_eq!(
            TABLE_RULE,
            "table{border-collapse:collapse;margin:1rem 0;width:100%}"
        );
        assert_eq!(TBODY_RULE, "tbody{display:table-row-group}");
        assert_eq!(
            DETAILS_RULE,
            "details{margin:0 0 .7rem;color:#134e4a}summary{display:block;cursor:pointer;font-weight:700;color:#134e4a;list-style:none}details[open]>summary{list-style:none}"
        );
        assert_eq!(
            ABBR_RULE,
            "abbr[title],acronym[title]{text-decoration:underline dotted #134e4a;text-underline-offset:.15em;cursor:help}"
        );
        assert_eq!(
            HEADING_RULE,
            "h1{font-size:1.75rem;line-height:1.2;margin:0 0 .6rem;letter-spacing:-.02em;padding-bottom:.4rem;border-bottom:3px solid #134e4a}h2{font-size:1.15rem;line-height:1.2;margin:1.1rem 0 .45rem;color:#134e4a;padding-bottom:.2rem;border-bottom:2px solid #0d9488}"
        );
        assert_eq!(
            SELECTION_RULE,
            "::selection{background:#ccfbf1;color:#134e4a}::-moz-selection{background:#ccfbf1;color:#134e4a}"
        );
        assert_eq!(LINK_RULE, "a{color:#0d9488;text-decoration:underline}");
        assert_eq!(LINK_VISITED_RULE, "a:visited{color:#0d9488}");
        assert_eq!(LINK_HOVER_RULE, "a:hover{color:#134e4a}");
        assert_eq!(LINK_FOCUS_RULE, "a:focus{color:#134e4a}");
        assert_eq!(PAGE_RULE, "@page{size:A4;margin:2.5rem 2.75rem}");
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48, "48rem measure must not regress");
        assert_eq!(CODE_FONT, "Consolas", "Consolas must not regress");
        assert_eq!(CODE_BEFORE_HU, 720, "pre margin-top .6rem must not regress");
        assert_eq!(CODE_AFTER_HU, 1200, "pre margin-bottom 1rem must not regress");
        assert_eq!(
            BODY_FG,
            Color {
                r: 19,
                g: 78,
                b: 74,
                a: 255
            }
        );
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * docagent_model::HU_PER_POINT);
        assert_eq!(LETTER_IMAGE_PATH, "docs/assets/mark.png");
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
        let doc = read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
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
        let decimals: Vec<_> = doc.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p)
                    if p.numbering
                        .as_ref()
                        .is_some_and(|n| n.format == Some(NumberFormat::Decimal)) =>
                {
                    Some(p)
                }
                _ => None,
            })
            .collect();
        assert!(
            decimals
                .iter()
                .any(|p| p.plain_text().contains("Hash the input"))
                && decimals
                    .iter()
                    .any(|p| p.plain_text().contains("Run Convert"))
                && decimals
                    .iter()
                    .any(|p| p.plain_text().contains("Compare the capsule")),
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
                .flat_map(|p| p.runs.iter().map(|r| (r.style.color, r.display_text().to_string())))
                .collect::<Vec<_>>()
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(
            back.contains("![Harbor mark](docs/assets/mark.png)"),
            "letter.md image path must round-trip: {back}"
        );
        assert!(
            back.contains("```") && back.contains("docagent prove examples/letter.md"),
            "markdown write must keep the prove fence: {back}"
        );
        assert!(
            back.contains("1. Hash the input") && back.contains("2. Run Convert"),
            "markdown write must keep the agent-replay ol: {back}"
        );
        assert!(back.contains("`prove`"), "{back}");
        assert!(
            back.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "{back}"
        );
        assert!(
            !back.contains("pre code{padding:.1em .35em")
                && !back.contains("pre code{background:#ccfbf1")
                && !back.contains("::marker{color:black")
                && !back.contains("::marker{color:CanvasText")
                && !back.contains("pre{padding:0}")
                && !back.contains("code{padding:0}")
                && !back.contains("code{font-family:monospace}"),
            "markdown write must not emit inherited pre-code chip pad, UA black markers, or unpadded pre/code: {back}"
        );
    }

    #[test]
    fn letter_md_hr_rule_keeps_landed() {
        assert_eq!(
            HR_RULE,
            "hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"
        );
        assert!(
            HR_RULE.contains("border:none")
                && HR_RULE.contains("border-top:3px solid #134e4a")
                && HR_RULE.contains("margin:1.1rem 0"),
            "hr must keep letter 3px body-teal rule and 1.1rem gap, not UA inset gray"
        );
        assert_ne!(
            HR_RULE, "hr{color:gray;border-style:inset;border-width:1px;margin:0.5em auto}",
            "WeasyPrint UA 1px inset gray hr is a sparse hairline, not letter.html 3px teal"
        );
        assert_ne!(
            HR_RULE, "hr{border:1px inset}",
            "UA 1px inset hr punches a gray band between the hop table and confirm list"
        );
        assert_ne!(
            HR_RULE, "hr{margin:.5em auto}",
            "UA 0.5em auto hr margin is sparse, not letter 1.1rem"
        );
        assert_eq!(
            PRE_CODE_RULE,
            "pre code{background:transparent;padding:0;color:#134e4a;font-family:ui-monospace,Consolas,monospace}"
        );
        assert_eq!(
            PRE_RULE,
            "pre{background:#ccfbf1;padding:.75rem 1rem;margin:.6rem 0 1rem;border-radius:4px;overflow:auto;font-family:ui-monospace,Consolas,monospace}"
        );
        assert_eq!(
            CODE_RULE,
            "code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"
        );
        assert_eq!(MARKER_RULE, "::marker{color:#134e4a}");
        assert_eq!(LIST_RULE, "ul,ol{margin:0 0 1rem;padding-left:1.25rem}");
        assert_eq!(
            TABLE_RULE,
            "table{border-collapse:collapse;margin:1rem 0;width:100%}"
        );
        assert_eq!(TBODY_RULE, "tbody{display:table-row-group}");
        assert_eq!(
            DETAILS_RULE,
            "details{margin:0 0 .7rem;color:#134e4a}summary{display:block;cursor:pointer;font-weight:700;color:#134e4a;list-style:none}details[open]>summary{list-style:none}"
        );
        assert_eq!(
            ABBR_RULE,
            "abbr[title],acronym[title]{text-decoration:underline dotted #134e4a;text-underline-offset:.15em;cursor:help}"
        );
        assert_eq!(
            HEADING_RULE,
            "h1{font-size:1.75rem;line-height:1.2;margin:0 0 .6rem;letter-spacing:-.02em;padding-bottom:.4rem;border-bottom:3px solid #134e4a}h2{font-size:1.15rem;line-height:1.2;margin:1.1rem 0 .45rem;color:#134e4a;padding-bottom:.2rem;border-bottom:2px solid #0d9488}"
        );
        assert_eq!(
            SELECTION_RULE,
            "::selection{background:#ccfbf1;color:#134e4a}::-moz-selection{background:#ccfbf1;color:#134e4a}"
        );
        assert_eq!(LINK_RULE, "a{color:#0d9488;text-decoration:underline}");
        assert_eq!(LINK_VISITED_RULE, "a:visited{color:#0d9488}");
        assert_eq!(LINK_HOVER_RULE, "a:hover{color:#134e4a}");
        assert_eq!(LINK_FOCUS_RULE, "a:focus{color:#134e4a}");
        assert_eq!(PAGE_RULE, "@page{size:A4;margin:2.5rem 2.75rem}");
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48, "48rem measure must not regress");
        assert_eq!(CODE_FONT, "Consolas", "Consolas must not regress");
        assert_eq!(
            BODY_FG,
            Color {
                r: 19,
                g: 78,
                b: 74,
                a: 255
            }
        );
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * docagent_model::HU_PER_POINT);
        assert_eq!(LETTER_IMAGE_PATH, "docs/assets/mark.png");
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
        let doc = read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        assert!(
            doc.sections[0]
                .body
                .iter()
                .any(|b| matches!(b, Block::Break(BreakKind::Thematic))),
            "letter.md IR must keep BreakKind::Thematic under hr 3px teal"
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
            "letter.md prove fence must stay Consolas under hr 3px teal"
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(
            back.contains("![Harbor mark](docs/assets/mark.png)"),
            "letter.md image path must round-trip: {back}"
        );
        assert!(
            back.contains("---"),
            "markdown write must keep the thematic break: {back}"
        );
        assert!(
            back.contains("```") && back.contains("docagent prove examples/letter.md"),
            "markdown write must keep the prove fence: {back}"
        );
        assert!(back.contains("`prove`"), "{back}");
        assert!(
            back.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "{back}"
        );
        assert!(
            !back.contains("hr{border:1px")
                && !back.contains("hr{border-style:inset")
                && !back.contains("hr{margin:.5em")
                && !back.contains("pre code{padding:.1em .35em")
                && !back.contains("pre code{background:#ccfbf1")
                && !back.contains("::marker{color:black")
                && !back.contains("::marker{color:CanvasText")
                && !back.contains("pre{padding:0}")
                && !back.contains("code{padding:0}")
                && !back.contains("code{font-family:monospace}"),
            "markdown write must not emit UA 1px inset hr, inherited pre-code chip pad, UA black markers, or unpadded pre/code: {back}"
        );
    }

    #[test]
    fn letter_md_li_margin_keeps_landed() {
        assert_eq!(LI_RULE, "li{margin:0 0 .35rem}");
        assert!(
            LI_RULE.contains("margin:0 0 .35rem") && LI_RULE.contains("li{"),
            "li must keep letter 0.35rem after-gap, not UA jammed list-item"
        );
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
        assert_eq!(
            LIST_ITEM_AFTER_HU, 420,
            "0.35rem at 16px rem / 96dpi is 420 HU"
        );
        assert_ne!(
            LIST_ITEM_AFTER_HU, 80,
            "IR list space_after 80 is jammed vs letter CSS 0.35rem"
        );
        assert_eq!(
            HR_RULE,
            "hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"
        );
        assert_eq!(
            PRE_CODE_RULE,
            "pre code{background:transparent;padding:0;color:#134e4a;font-family:ui-monospace,Consolas,monospace}"
        );
        assert_eq!(
            PRE_RULE,
            "pre{background:#ccfbf1;padding:.75rem 1rem;margin:.6rem 0 1rem;border-radius:4px;overflow:auto;font-family:ui-monospace,Consolas,monospace}"
        );
        assert_eq!(
            CODE_RULE,
            "code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"
        );
        assert_eq!(MARKER_RULE, "::marker{color:#134e4a}");
        assert_eq!(LIST_RULE, "ul,ol{margin:0 0 1rem;padding-left:1.25rem}");
        assert_eq!(
            TABLE_RULE,
            "table{border-collapse:collapse;margin:1rem 0;width:100%}"
        );
        assert_eq!(TBODY_RULE, "tbody{display:table-row-group}");
        assert_eq!(
            DETAILS_RULE,
            "details{margin:0 0 .7rem;color:#134e4a}summary{display:block;cursor:pointer;font-weight:700;color:#134e4a;list-style:none}details[open]>summary{list-style:none}"
        );
        assert_eq!(
            ABBR_RULE,
            "abbr[title],acronym[title]{text-decoration:underline dotted #134e4a;text-underline-offset:.15em;cursor:help}"
        );
        assert_eq!(
            HEADING_RULE,
            "h1{font-size:1.75rem;line-height:1.2;margin:0 0 .6rem;letter-spacing:-.02em;padding-bottom:.4rem;border-bottom:3px solid #134e4a}h2{font-size:1.15rem;line-height:1.2;margin:1.1rem 0 .45rem;color:#134e4a;padding-bottom:.2rem;border-bottom:2px solid #0d9488}"
        );
        assert_eq!(
            SELECTION_RULE,
            "::selection{background:#ccfbf1;color:#134e4a}::-moz-selection{background:#ccfbf1;color:#134e4a}"
        );
        assert_eq!(LINK_RULE, "a{color:#0d9488;text-decoration:underline}");
        assert_eq!(LINK_VISITED_RULE, "a:visited{color:#0d9488}");
        assert_eq!(LINK_HOVER_RULE, "a:hover{color:#134e4a}");
        assert_eq!(LINK_FOCUS_RULE, "a:focus{color:#134e4a}");
        assert_eq!(PAGE_RULE, "@page{size:A4;margin:2.5rem 2.75rem}");
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48, "48rem measure must not regress");
        assert_eq!(CODE_FONT, "Consolas", "Consolas must not regress");
        assert_eq!(
            BODY_FG,
            Color {
                r: 19,
                g: 78,
                b: 74,
                a: 255
            }
        );
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * docagent_model::HU_PER_POINT);
        assert_eq!(LETTER_IMAGE_PATH, "docs/assets/mark.png");
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
        assert!(
            text.contains("docagent prove examples/letter.md"),
            "letter.md must keep the fenced prove command WeasyPrint paints in pre code"
        );
        let doc = read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let items: Vec<_> = doc.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) if p.numbering.is_some() => Some(p),
                _ => None,
            })
            .collect();
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
        assert!(
            items.iter().any(|p| {
                p.plain_text().contains("Receiving dock") && p.space_after == LIST_ITEM_AFTER_HU
            }),
            "letter.md Receiving dock must keep li 0.35rem before Bay 4"
        );
        assert!(
            doc.sections[0]
                .body
                .iter()
                .any(|b| matches!(b, Block::Break(BreakKind::Thematic))),
            "letter.md IR must keep BreakKind::Thematic under hr 3px teal"
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
            "letter.md prove fence must stay Consolas under li 0.35rem"
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(
            back.contains("![Harbor mark](docs/assets/mark.png)"),
            "letter.md image path must round-trip: {back}"
        );
        assert!(
            back.contains("1. Hash the input") && back.contains("- [x] Receiving dock is clear"),
            "markdown write must keep the agent-replay ol and task list: {back}"
        );
        assert!(
            back.contains("```") && back.contains("docagent prove examples/letter.md"),
            "markdown write must keep the prove fence: {back}"
        );
        assert!(back.contains("`prove`"), "{back}");
        assert!(
            back.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "{back}"
        );
        assert!(
            !back.contains("li{margin:0;}")
                && !back.contains("li{margin:1em")
                && !back.contains("li{display:list-item}")
                && !back.contains("hr{border:1px")
                && !back.contains("hr{border-style:inset")
                && !back.contains("pre code{padding:.1em .35em")
                && !back.contains("pre code{background:#ccfbf1")
                && !back.contains("::marker{color:black")
                && !back.contains("::marker{color:CanvasText")
                && !back.contains("pre{padding:0}")
                && !back.contains("code{padding:0}")
                && !back.contains("code{font-family:monospace}"),
            "markdown write must not emit UA jammed li, 1px inset hr, inherited pre-code chip pad, UA black markers, or unpadded pre/code: {back}"
        );
    }

    #[test]
    fn letter_md_tasks_rule_keeps_landed() {
        assert_eq!(
            TASKS_RULE,
            "ul.tasks{list-style:none;padding-left:1.25rem}ul.tasks>li{list-style:none}ul.tasks ul{list-style:disc}ul.tasks input[type=checkbox]{appearance:none;-webkit-appearance:none;display:inline-block;width:.9em;height:.9em;margin-right:.4rem;vertical-align:middle;border:1.5pt solid #134e4a;border-radius:2px;background:#fff;accent-color:#134e4a}ul.tasks input[type=checkbox]:checked{background:#134e4a}"
        );
        assert!(
            TASKS_RULE.contains("ul.tasks{list-style:none;padding-left:1.25rem}")
                && TASKS_RULE.contains("ul.tasks>li{list-style:none}")
                && TASKS_RULE.contains("ul.tasks ul{list-style:disc}")
                && TASKS_RULE.contains("border:1.5pt solid #134e4a")
                && TASKS_RULE.contains("ul.tasks input[type=checkbox]:checked{background:#134e4a}"),
            "task ul must keep no-disc 1.25rem, nested Bay 4 disc, and 1.5pt teal checkbox chip"
        );
        assert_ne!(
            TASKS_RULE, "ul.tasks{list-style:disc;padding-left:40px}",
            "WeasyPrint UA disc + 40px gutter paints beside GFM checkboxes"
        );
        assert_ne!(
            TASKS_RULE, "ul{list-style:disc;padding-left:40px}",
            "UA ul disc/40px is not letter.html ul.tasks 1.25rem no-disc"
        );
        assert_eq!(LIST_INDENT_HU, 1500, "1.25rem at 16px rem / 96dpi is 1500 HU");
        assert_ne!(
            LIST_INDENT_HU, 3000,
            "UA 40px at 96dpi is 3000 HU, not letter 1.25rem"
        );
        assert_eq!(LI_RULE, "li{margin:0 0 .35rem}");
        assert_eq!(
            LIST_ITEM_AFTER_HU, 420,
            "0.35rem at 16px rem / 96dpi is 420 HU"
        );
        assert_eq!(
            HR_RULE,
            "hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"
        );
        assert_eq!(
            PRE_CODE_RULE,
            "pre code{background:transparent;padding:0;color:#134e4a;font-family:ui-monospace,Consolas,monospace}"
        );
        assert_eq!(
            PRE_RULE,
            "pre{background:#ccfbf1;padding:.75rem 1rem;margin:.6rem 0 1rem;border-radius:4px;overflow:auto;font-family:ui-monospace,Consolas,monospace}"
        );
        assert_eq!(
            CODE_RULE,
            "code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"
        );
        assert_eq!(MARKER_RULE, "::marker{color:#134e4a}");
        assert_eq!(LIST_RULE, "ul,ol{margin:0 0 1rem;padding-left:1.25rem}");
        assert_eq!(
            TABLE_RULE,
            "table{border-collapse:collapse;margin:1rem 0;width:100%}"
        );
        assert_eq!(TBODY_RULE, "tbody{display:table-row-group}");
        assert_eq!(
            DETAILS_RULE,
            "details{margin:0 0 .7rem;color:#134e4a}summary{display:block;cursor:pointer;font-weight:700;color:#134e4a;list-style:none}details[open]>summary{list-style:none}"
        );
        assert_eq!(
            ABBR_RULE,
            "abbr[title],acronym[title]{text-decoration:underline dotted #134e4a;text-underline-offset:.15em;cursor:help}"
        );
        assert_eq!(
            HEADING_RULE,
            "h1{font-size:1.75rem;line-height:1.2;margin:0 0 .6rem;letter-spacing:-.02em;padding-bottom:.4rem;border-bottom:3px solid #134e4a}h2{font-size:1.15rem;line-height:1.2;margin:1.1rem 0 .45rem;color:#134e4a;padding-bottom:.2rem;border-bottom:2px solid #0d9488}"
        );
        assert_eq!(
            SELECTION_RULE,
            "::selection{background:#ccfbf1;color:#134e4a}::-moz-selection{background:#ccfbf1;color:#134e4a}"
        );
        assert_eq!(LINK_RULE, "a{color:#0d9488;text-decoration:underline}");
        assert_eq!(LINK_VISITED_RULE, "a:visited{color:#0d9488}");
        assert_eq!(LINK_HOVER_RULE, "a:hover{color:#134e4a}");
        assert_eq!(LINK_FOCUS_RULE, "a:focus{color:#134e4a}");
        assert_eq!(PAGE_RULE, "@page{size:A4;margin:2.5rem 2.75rem}");
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48, "48rem measure must not regress");
        assert_eq!(CODE_FONT, "Consolas", "Consolas must not regress");
        assert_eq!(
            BODY_FG,
            Color {
                r: 19,
                g: 78,
                b: 74,
                a: 255
            }
        );
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * docagent_model::HU_PER_POINT);
        assert_eq!(LETTER_IMAGE_PATH, "docs/assets/mark.png");
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
        assert!(
            text.contains("docagent prove examples/letter.md"),
            "letter.md must keep the fenced prove command WeasyPrint paints in pre code"
        );
        let doc = read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let items: Vec<_> = doc.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) if p.numbering.is_some() => Some(p),
                _ => None,
            })
            .collect();
        assert!(
            items.iter().any(|p| {
                p.plain_text().contains("Receiving dock")
                    && p.indent_left == LIST_INDENT_HU
                    && p.space_after == LIST_ITEM_AFTER_HU
                    && p.numbering
                        .as_ref()
                        .is_some_and(|n| n.format == Some(NumberFormat::Task) && n.start == Some(1))
            }),
            "letter.md Receiving dock must stay NumberFormat::Task at 1.25rem / 0.35rem: {:?}",
            items
                .iter()
                .map(|p| (p.plain_text(), p.indent_left, p.space_after))
                .collect::<Vec<_>>()
        );
        assert!(
            items.iter().any(|p| {
                p.plain_text().contains("Replay the convert")
                    && p.indent_left == LIST_INDENT_HU
                    && p.numbering.as_ref().is_some_and(|n| {
                        n.format == Some(NumberFormat::Task) && n.start.is_none()
                    })
            }),
            "letter.md Replay must stay unchecked NumberFormat::Task at 1.25rem"
        );
        assert!(
            items.iter().any(|p| {
                p.plain_text().contains("Bay 4")
                    && p.numbering
                        .as_ref()
                        .is_some_and(|n| n.format == Some(NumberFormat::Bullet))
            }),
            "letter.md Bay 4 nest must stay a disc under ul.tasks, not a checkbox"
        );
        assert!(
            doc.sections[0]
                .body
                .iter()
                .any(|b| matches!(b, Block::Break(BreakKind::Thematic))),
            "letter.md IR must keep BreakKind::Thematic under hr 3px teal"
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
            "letter.md prove fence must stay Consolas under ul.tasks"
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(
            back.contains("![Harbor mark](docs/assets/mark.png)"),
            "letter.md image path must round-trip: {back}"
        );
        assert!(
            back.contains("- [x] Receiving dock is clear")
                && back.contains("- [ ] Replay the convert instead of hoping"),
            "markdown write must keep the GFM task list: {back}"
        );
        assert!(
            back.contains("```") && back.contains("docagent prove examples/letter.md"),
            "markdown write must keep the prove fence: {back}"
        );
        assert!(back.contains("`prove`"), "{back}");
        assert!(
            back.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "{back}"
        );
        assert!(
            !back.contains("ul.tasks{list-style:disc")
                && !back.contains("ul.tasks{padding-left:40px")
                && !back.contains("ul.tasks input{display:none")
                && !back.contains("li{margin:0;}")
                && !back.contains("li{margin:1em")
                && !back.contains("li{display:list-item}")
                && !back.contains("hr{border:1px")
                && !back.contains("hr{border-style:inset")
                && !back.contains("pre code{padding:.1em .35em")
                && !back.contains("pre code{background:#ccfbf1")
                && !back.contains("::marker{color:black")
                && !back.contains("::marker{color:CanvasText")
                && !back.contains("pre{padding:0}")
                && !back.contains("code{padding:0}")
                && !back.contains("code{font-family:monospace}"),
            "markdown write must not emit UA disc tasks, 40px task gutter, hidden checkboxes, jammed li, 1px inset hr, inherited pre-code chip pad, UA black markers, or unpadded pre/code: {back}"
        );
    }

    #[test]
    fn letter_md_cell_pad_keeps_landed() {
        assert_eq!(
            CELL_RULE,
            "th,td{border:1px solid #cbd5e1;padding:.4rem .65rem;text-align:left;font-size:.95rem}"
        );
        assert!(
            CELL_RULE.contains("padding:.4rem .65rem")
                && CELL_RULE.contains("border:1px solid #cbd5e1")
                && CELL_RULE.contains("text-align:left")
                && CELL_RULE.contains("font-size:.95rem"),
            "hop cells must keep letter 0.4rem 0.65rem pad, 1px slate border, left align, .95rem type"
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
        assert_eq!(
            CELL_PAD_X_HU, 780,
            "0.65rem at 16px rem / 96dpi is 780 HU"
        );
        assert_eq!(
            CELL_PAD_Y_HU, 480,
            "0.4rem at 16px rem / 96dpi is 480 HU"
        );
        assert_ne!(
            CELL_PAD_X_HU, 75,
            "UA 1px at 96dpi is 75 HU, not letter 0.65rem"
        );
        assert_ne!(
            CELL_PAD_Y_HU, 75,
            "UA 1px at 96dpi is 75 HU, not letter 0.4rem"
        );
        assert_eq!(
            TASKS_RULE,
            "ul.tasks{list-style:none;padding-left:1.25rem}ul.tasks>li{list-style:none}ul.tasks ul{list-style:disc}ul.tasks input[type=checkbox]{appearance:none;-webkit-appearance:none;display:inline-block;width:.9em;height:.9em;margin-right:.4rem;vertical-align:middle;border:1.5pt solid #134e4a;border-radius:2px;background:#fff;accent-color:#134e4a}ul.tasks input[type=checkbox]:checked{background:#134e4a}"
        );
        assert_eq!(LI_RULE, "li{margin:0 0 .35rem}");
        assert_eq!(
            LIST_ITEM_AFTER_HU, 420,
            "0.35rem at 16px rem / 96dpi is 420 HU"
        );
        assert_eq!(
            HR_RULE,
            "hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}"
        );
        assert_eq!(
            PRE_CODE_RULE,
            "pre code{background:transparent;padding:0;color:#134e4a;font-family:ui-monospace,Consolas,monospace}"
        );
        assert_eq!(
            PRE_RULE,
            "pre{background:#ccfbf1;padding:.75rem 1rem;margin:.6rem 0 1rem;border-radius:4px;overflow:auto;font-family:ui-monospace,Consolas,monospace}"
        );
        assert_eq!(
            CODE_RULE,
            "code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}"
        );
        assert_eq!(MARKER_RULE, "::marker{color:#134e4a}");
        assert_eq!(LIST_RULE, "ul,ol{margin:0 0 1rem;padding-left:1.25rem}");
        assert_eq!(
            TABLE_RULE,
            "table{border-collapse:collapse;margin:1rem 0;width:100%}"
        );
        assert_eq!(TBODY_RULE, "tbody{display:table-row-group}");
        assert_eq!(
            DETAILS_RULE,
            "details{margin:0 0 .7rem;color:#134e4a}summary{display:block;cursor:pointer;font-weight:700;color:#134e4a;list-style:none}details[open]>summary{list-style:none}"
        );
        assert_eq!(
            ABBR_RULE,
            "abbr[title],acronym[title]{text-decoration:underline dotted #134e4a;text-underline-offset:.15em;cursor:help}"
        );
        assert_eq!(
            HEADING_RULE,
            "h1{font-size:1.75rem;line-height:1.2;margin:0 0 .6rem;letter-spacing:-.02em;padding-bottom:.4rem;border-bottom:3px solid #134e4a}h2{font-size:1.15rem;line-height:1.2;margin:1.1rem 0 .45rem;color:#134e4a;padding-bottom:.2rem;border-bottom:2px solid #0d9488}"
        );
        assert_eq!(
            SELECTION_RULE,
            "::selection{background:#ccfbf1;color:#134e4a}::-moz-selection{background:#ccfbf1;color:#134e4a}"
        );
        assert_eq!(LINK_RULE, "a{color:#0d9488;text-decoration:underline}");
        assert_eq!(LINK_VISITED_RULE, "a:visited{color:#0d9488}");
        assert_eq!(LINK_HOVER_RULE, "a:hover{color:#134e4a}");
        assert_eq!(LINK_FOCUS_RULE, "a:focus{color:#134e4a}");
        assert_eq!(PAGE_RULE, "@page{size:A4;margin:2.5rem 2.75rem}");
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48, "48rem measure must not regress");
        assert_eq!(CODE_FONT, "Consolas", "Consolas must not regress");
        assert_eq!(
            BODY_FG,
            Color {
                r: 19,
                g: 78,
                b: 74,
                a: 255
            }
        );
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * docagent_model::HU_PER_POINT);
        assert_eq!(LETTER_IMAGE_PATH, "docs/assets/mark.png");
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
        assert!(
            text.contains("- [x] Receiving dock is clear"),
            "letter.md must keep the task list WeasyPrint paints as ul.tasks"
        );
        assert!(
            text.contains("docagent prove examples/letter.md"),
            "letter.md must keep the fenced prove command WeasyPrint paints in pre code"
        );
        let doc = read(&md).expect("docagent-md read letter.md");
        let img = first_image(&doc);
        assert!(
            img.width >= MIN_IMAGE_EDGE_HU && img.height >= MIN_IMAGE_EDGE_HU,
            "24pt img floor must not regress"
        );
        let table = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Table(t) => Some(t),
                _ => None,
            })
            .expect("letter table");
        assert_eq!(table.rows[0].cells[0].shading, Some(TH_BG));
        for row in &table.rows {
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
        let hop = table.rows[0].cells[0]
            .blocks
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) if p.plain_text().contains("Hop") => Some(p),
                _ => None,
            })
            .expect("Hop");
        assert!(
            hop.runs.iter().any(|r| r.style.bold && r.style.color == TH_FG),
            "letter.md Hop header must stay bold #134e4a under CELL_RULE"
        );
        assert!(
            doc.sections[0]
                .body
                .iter()
                .any(|b| matches!(b, Block::Break(BreakKind::Thematic))),
            "letter.md IR must keep BreakKind::Thematic under hr 3px teal"
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
            "letter.md prove fence must stay Consolas under cell pad .4rem .65rem"
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(
            back.contains("![Harbor mark](docs/assets/mark.png)"),
            "letter.md image path must round-trip: {back}"
        );
        assert!(
            back.contains("| **Hop** | **File** | **Proof** |")
                && back.contains("| Input | letter.md | input SHA-256 |"),
            "markdown write must keep the hop table: {back}"
        );
        assert!(
            back.contains("- [x] Receiving dock is clear")
                && back.contains("- [ ] Replay the convert instead of hoping"),
            "markdown write must keep the GFM task list: {back}"
        );
        assert!(
            back.contains("```") && back.contains("docagent prove examples/letter.md"),
            "markdown write must keep the prove fence: {back}"
        );
        assert!(back.contains("`prove`"), "{back}");
        assert!(
            back.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "{back}"
        );
        assert!(
            !back.contains("th,td{padding:1px")
                && !back.contains("td,th{padding:1px")
                && !back.contains("th,td{padding:0}")
                && !back.contains("th,td{border:none")
                && !back.contains("ul.tasks{list-style:disc")
                && !back.contains("ul.tasks{padding-left:40px")
                && !back.contains("ul.tasks input{display:none")
                && !back.contains("li{margin:0;}")
                && !back.contains("li{margin:1em")
                && !back.contains("li{display:list-item}")
                && !back.contains("hr{border:1px")
                && !back.contains("hr{border-style:inset")
                && !back.contains("pre code{padding:.1em .35em")
                && !back.contains("pre code{background:#ccfbf1")
                && !back.contains("::marker{color:black")
                && !back.contains("::marker{color:CanvasText")
                && !back.contains("pre{padding:0}")
                && !back.contains("code{padding:0}")
                && !back.contains("code{font-family:monospace}"),
            "markdown write must not emit UA 1px cell pad, no-border hop grid, disc tasks, 40px task gutter, hidden checkboxes, jammed li, 1px inset hr, inherited pre-code chip pad, UA black markers, or unpadded pre/code: {back}"
        );
    }

    #[test]
    fn letter_md_img_24pt_floor_and_article_css_keep_landed() {
        assert_eq!(
            IMG_RULE,
            "img{min-width:24pt;min-height:24pt;max-width:100%;height:auto;margin:.6rem 0;display:block}"
        );
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
                && ARTICLE_RULE.contains("color:#134e4a"),
            "article CSS must stay a readable WeasyPrint letter column"
        );
        assert_eq!(ARTICLE_MAX_WIDTH_REM, 48, "48rem measure must not regress");
        assert_ne!(
            ARTICLE_MAX_WIDTH_REM, 100,
            "100rem is a full-bleed column, not letter.html 48rem"
        );
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * docagent_model::HU_PER_POINT);
        assert_eq!(LETTER_IMAGE_PATH, "docs/assets/mark.png");
        assert_eq!(PAGE_RULE, "@page{size:A4;margin:2.5rem 2.75rem}");
        assert_eq!(PRE_RULE.as_bytes()[0], b'p');
        assert_eq!(CODE_RULE.as_bytes()[0], b'c');
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
        let md = std::fs::read(&path).expect("examples/letter.md");
        let text = std::str::from_utf8(&md).expect("letter.md utf-8");
        assert!(
            text.contains("![Harbor mark](docs/assets/mark.png)"),
            "letter.md must keep the relative harbor-mark path"
        );
        let doc = read(&md).expect("docagent-md read letter.md");
        let png = fixture_png();
        let img = first_image(&doc);
        assert_eq!(img.bytes, png);
        assert_eq!(
            (img.width, img.height),
            (MIN_IMAGE_EDGE_HU, MIN_IMAGE_EDGE_HU),
            "letter.md harbor mark IR must be the PDF 24pt floor, not 16px at 96dpi"
        );
        let img_p = first_image_paragraph(&doc);
        assert_eq!(
            img_p.space_after, IMAGE_AFTER_HU,
            "gap after mark must stay letter CSS 0.7rem"
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(
            back.contains("![Harbor mark](docs/assets/mark.png)"),
            "markdown image path must round-trip: {back}"
        );
        assert!(
            !back.contains("data:image/png;base64,"),
            "letter.md path round-trip must not emit a data URI: {back}"
        );
        let again = read(back.as_bytes()).expect("round-trip md");
        assert_eq!(first_image(&again).bytes, png);
        assert_eq!(
            (first_image(&again).width, first_image(&again).height),
            (MIN_IMAGE_EDGE_HU, MIN_IMAGE_EDGE_HU),
            "path round-trip must keep the PDF 24pt floor"
        );
        let twice = String::from_utf8(write(&again).unwrap()).unwrap();
        assert!(
            twice.contains("![Harbor mark](docs/assets/mark.png)"),
            "second write must keep the relative path: {twice}"
        );
        assert!(back.contains("`prove`"), "{back}");
        assert!(
            back.contains("[DocAgent](https://github.com/kevin9327/docagent)"),
            "{back}"
        );
        assert!(
            !back.contains("img{min-width:12pt")
                && !back.contains("article{max-width:100rem")
                && !back.contains("color:#111827"),
            "markdown write must not emit a 12pt speck or full-bleed article: {back}"
        );
    }

    #[test]
    fn sniff_rejects_ole_and_zip() {
        assert!(!sniff(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]));
        assert!(!sniff(b"PK\x03\x04rest"));
        assert!(sniff(b"# Hello\n"));
    }
}
