//! Markdown ↔ IR. Headings, paragraphs, and pipe tables.
//!
//! This is a document interchange codec, not a full CommonMark parser.

#![forbid(unsafe_code)]

use docagent_model::{
    Block, BreakKind, Document, ImageData, InlineObject, NumberFormat, NumberingRef, Paragraph,
    Run, RunContent, Section, Table, WrapMode,
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
    if bytes.starts_with(b"HWP Document File") || bytes.starts_with(b"<?xml") || bytes.starts_with(b"{")
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
            section.body.push(Block::Paragraph(heading(rest, 1, 1800, 200, 400)));
        } else if let Some(rest) = content.strip_prefix("## ") {
            section.body.push(Block::Paragraph(heading(rest, 2, 1400, 360, 240)));
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
            section.body.push(Block::Paragraph(decimal_item(rest, n, nest)));
        } else if let Some(rest) = content.strip_prefix("> ") {
            let mut p = paragraph_from_inlines(rest);
            p.quote = true;
            p.space_before = 80;
            p.space_after = 200;
            section.body.push(Block::Paragraph(p));
        } else {
            let mut p = paragraph_from_inlines(trimmed);
            p.space_after = 200;
            section.body.push(Block::Paragraph(p));
        }
    }
    flush_table(&mut section, &mut table_rows);
    if in_fence {
        flush_fence(&mut section, &mut fence_lines);
    }
    doc.sections.push(section);
    Ok(doc)
}

fn flush_fence(section: &mut Section, lines: &mut Vec<String>) {
    let body = lines.join(" ");
    lines.clear();
    let mut p = Paragraph::from_text(body);
    p.code_block = true;
    p.space_before = 80;
    p.space_after = 200;
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
    1440i32.saturating_mul(i32::from(nest) + 1)
}

fn paragraph_from_inlines(text: &str) -> Paragraph {
    let mut p = Paragraph::from_text("");
    p.runs = parse_runs(text);
    if p.runs.is_empty() {
        p.runs.push(Run::text(""));
    }
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
        if !marks.code && chars[i] == '['
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
    p.space_after = 80;
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
    p.space_after = 80;
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
    p.space_after = 80;
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
            for b in &mut cell.blocks {
                if let Block::Paragraph(p) = b {
                    let text = p.plain_text();
                    *p = paragraph_from_inlines(&text);
                    if header {
                        for run in &mut p.runs {
                            run.style.bold = true;
                        }
                    }
                }
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
                        _ if p.numbering.as_ref().is_some_and(|n| {
                            n.format == Some(NumberFormat::Decimal)
                        }) =>
                        {
                            let nref = p.numbering.as_ref().unwrap();
                            let i = nref.start.unwrap_or(1);
                            let pad = "  ".repeat(nref.level as usize);
                            out.push_str(&format!("{pad}{i}. {}\n", write_runs(p)));
                        }
                        _ if p.numbering.as_ref().is_some_and(|n| {
                            n.format == Some(NumberFormat::Task)
                        }) =>
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
                        let next_listed = section.body.get(i + 1).is_some_and(|b| {
                            matches!(b, Block::Paragraph(q) if q.numbering.is_some())
                        });
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
    s.push_str(&image_src(img));
    s.push(')');
    s
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

fn raster_hu(bytes: &[u8]) -> (docagent_model::Hu, docagent_model::Hu) {
    match png_px(bytes) {
        Some((w, h)) => {
            let px = docagent_model::HU_PER_INCH / 96;
            (
                i32::try_from(w).unwrap_or(i32::MAX).saturating_mul(px),
                i32::try_from(h).unwrap_or(i32::MAX).saturating_mul(px),
            )
        }
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
        let src = b"| Hop | File | Proof |\n| --- | --- | --- |\n| Input | letter.md | input SHA-256 |\n";
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
        assert!(h.runs.iter().any(|r| r.style.bold && matches!(&r.content, RunContent::Text(t) if t.contains("Hop"))));
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
            r.style.italic && matches!(&r.content, RunContent::Text(t) if t.contains("byte-for-byte"))
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
            .map(|r| (r.style.bold, r.style.italic, match &r.content {
                RunContent::Text(t) => t.as_str(),
                _ => "",
            }))
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
            r.style.strike && !r.style.subscript && matches!(&r.content, RunContent::Text(t) if t == "guess")
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
            r.style.highlight.is_some()
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
                (Some(NumberFormat::Task), Some(1), "Replay the convert".into()),
                (Some(NumberFormat::Task), None, "Hope".into()),
            ]
        );
        let back = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(back.contains("- [x] Replay the convert"), "{back}");
        assert!(back.contains("- [ ] Hope"), "{back}");
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
        assert!(back.contains("```\ndocagent prove examples/letter.md\n```"), "{back}");
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
        let titled = read(b"[DocAgent](https://github.com/kevin9327/docagent \"runtime\")\n").unwrap();
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
        assert!(p.runs.iter().any(|r| matches!(
            &r.content,
            RunContent::Inline(InlineObject::Image(_))
        )));
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
        assert!(text.contains("![mark](data:image/png;base64,"), "{text}");
        let again = read(&back).unwrap();
        assert_eq!(first_image(&again), &img);
        let twice = write(&again).unwrap();
        assert_eq!(first_image(&read(&twice).unwrap()), &img);
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
        assert_eq!(first_image(&read(untitled.as_bytes()).unwrap()).bytes, img.bytes);
        let titled = format!("![]({} 'caption')\n", image_src(&img));
        assert_eq!(
            first_image(&read(titled.as_bytes()).unwrap()).alt_text.as_deref(),
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
        assert!(back.contains("![mark](data:image/png;base64,"), "{back}");
        assert_eq!(first_image(&read(back.as_bytes()).unwrap()).bytes, png);
    }

    #[test]
    fn letter_md_reads_mark_png() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
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
    }

    #[test]
    fn headings_carry_size_for_layout() {
        let doc = read(b"# Title\n\n## Sub\n\nBody\n").unwrap();
        let h1 = match &doc.sections[0].body[0] {
            Block::Paragraph(p) => p,
            _ => panic!("h1"),
        };
        assert_eq!(h1.outline_level, Some(1));
        assert_eq!(h1.runs[0].style.size, 1800);
        assert!(h1.runs[0].style.bold);
    }

    #[test]
    fn sniff_rejects_ole_and_zip() {
        assert!(!sniff(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]));
        assert!(!sniff(b"PK\x03\x04rest"));
        assert!(sniff(b"# Hello\n"));
    }
}
