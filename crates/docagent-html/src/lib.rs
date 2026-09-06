//! Semantic HTML ↔ IR (not from the paint tree). Search/RAG/a11y.

#![forbid(unsafe_code)]

use docagent_model::{
    Block, BreakKind, Document, Float, HeaderFooter, ImageData, InlineObject, NumberFormat,
    NumberingRef, Paragraph, Run, RunContent, Section, Table, TableCell, TableRow, WrapMode,
    HU_PER_INCH,
};

pub fn to_html(doc: &Document) -> String {
    let mut s = String::from(
        r#"<!DOCTYPE html><html lang="en"><head><meta charset="utf-8"/><title>DocAgent</title><style>body{margin:0;background:#f8fafc;color:#111827}article{max-width:48rem;margin:2rem auto;padding:2.5rem 2.75rem;background:#fff;border:1px solid #e2e8f0;font-family:"Segoe UI",Roboto,Helvetica,Arial,sans-serif;line-height:1.45}h1{font-size:1.75rem;margin:0 0 .6rem;letter-spacing:-.02em;padding-bottom:.4rem;border-bottom:3px solid #134e4a}h2{font-size:1.15rem;margin:1.1rem 0 .45rem;color:#134e4a;padding-bottom:.2rem;border-bottom:2px solid #0d9488}p{margin:0 0 .7rem}img{max-width:100%;height:auto;margin:.6rem 0}blockquote{margin:.5rem 0 1rem;padding:.15rem 0 .15rem 1rem;border-left:4px solid #134e4a;color:#134e4a}strong{font-weight:700}em{font-style:italic}a{color:#0d9488}u{text-decoration:underline}sup{font-size:.75em;vertical-align:super}sub{font-size:.75em;vertical-align:sub}s{text-decoration:line-through;color:#134e4a}mark{background:#fde68a;padding:.05em .2em}code{background:#ccfbf1;padding:.1em .35em;border-radius:3px;font-family:ui-monospace,Consolas,monospace;font-size:.92em;color:#134e4a}pre{background:#ccfbf1;padding:.75rem 1rem;margin:.6rem 0 1rem;border-radius:4px;overflow:auto}pre code{background:transparent;padding:0;color:#134e4a}hr{border:none;border-top:3px solid #134e4a;margin:1.1rem 0}ul,ol{margin:0 0 1rem;padding-left:1.25rem}ul ul,ol ul,ul ol,ol ol{margin:.25rem 0}li{margin:0 0 .35rem}ul.tasks{list-style:none;padding-left:1.25rem}ul.tasks input{margin-right:.4rem;vertical-align:middle;accent-color:#134e4a}table{border-collapse:collapse;margin:1rem 0;width:100%}th,td{border:1px solid #cbd5e1;padding:.4rem .65rem;text-align:left;font-size:.95rem}th{background:#ccfbf1;color:#134e4a}</style></head><body>"#,
    );
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
            let start = attr(attrs, "start").and_then(|s| s.parse().ok()).unwrap_or(1);
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

fn paragraph_from_runs(
    runs: Vec<Run>,
    outline: Option<u8>,
    quote: bool,
    code: bool,
) -> Paragraph {
    let mut p = Paragraph::from_text("");
    p.runs = if runs.is_empty() {
        vec![Run::text("")]
    } else {
        runs
    };
    p.outline_level = outline;
    p.quote = quote;
    p.code_block = code;
    p
}

fn image_para(img: ImageData) -> Paragraph {
    let mut p = Paragraph::from_text("");
    p.runs = vec![image_run(img)];
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
        Some(Paragraph::from_text(text))
    }
}

fn aside_blocks(inner: Vec<Block>) -> Vec<Block> {
    if let [Block::Paragraph(p)] = inner.as_slice()
        && let [Run {
            content: RunContent::Inline(InlineObject::Image(img)),
            ..
        }] = p.runs.as_slice()
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
            "code" | "kbd" => self.style.code = true,
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
            "code" | "kbd" => self.style.code = false,
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
                    let display: String = inner
                        .runs
                        .iter()
                        .map(Run::display_text)
                        .collect();
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
                    let start = attr(&attrs, "start").and_then(|s| s.parse().ok()).unwrap_or(1);
                    nested.extend(parse_list(
                        cur,
                        fmt,
                        start,
                        list_level.saturating_add(1),
                    ));
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

fn parse_list(
    cur: &mut Cursor<'_>,
    fmt: NumberFormat,
    mut start: u32,
    level: u8,
) -> Vec<Block> {
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
                let st = attr(&attrs, "start").and_then(|s| s.parse().ok()).unwrap_or(1);
                blocks.extend(parse_list(cur, nfmt, st, level.saturating_add(1)));
            }
            Some(Token::Open {
                name,
                self_closing,
                ..
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

fn parse_li_blocks(
    cur: &mut Cursor<'_>,
    fmt: NumberFormat,
    start: u32,
    level: u8,
) -> Vec<Block> {
    let inner = parse_inlines_at(cur, "li", level);
    let mut p = paragraph_from_runs(inner.runs, None, false, false);
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
                name,
                self_closing,
                ..
            }) if name == "tr" => {
                if !self_closing {
                    rows.push(parse_tr(cur, false));
                }
            }
            Some(Token::Open {
                name,
                self_closing,
                ..
            }) if matches!(name.as_str(), "thead" | "tbody" | "tfoot") => {
                if !self_closing {
                    parse_table_rows(cur, &name, &mut rows, name == "thead");
                }
            }
            Some(Token::Open {
                name,
                self_closing,
                ..
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
                name,
                self_closing,
                ..
            }) if name == "tr" => {
                if !self_closing {
                    rows.push(parse_tr(cur, header_section));
                }
            }
            Some(Token::Open {
                name,
                self_closing,
                ..
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
                name,
                self_closing,
                ..
            }) if name == "th" || name == "td" => {
                if name == "th" {
                    header = true;
                }
                let p = para_until(cur, &name, None, false, false, self_closing);
                cells.push(TableCell {
                    blocks: vec![Block::Paragraph(p)],
                    ..TableCell::default()
                });
            }
            Some(Token::Open {
                name,
                self_closing,
                ..
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
    let (width, height) = raster_hu(&bytes);
    let alt = attr(attrs, "alt").filter(|s| !s.is_empty()).map(str::to_string);
    Some(ImageData {
        bytes,
        mime,
        width,
        height,
        alt_text: alt,
        wrap: WrapMode::Inline,
    })
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

fn raster_hu(bytes: &[u8]) -> (i32, i32) {
    match png_ihdr_px(bytes) {
        Some((w, h)) => (px_to_hu(w), px_to_hu(h)),
        None => (HU_PER_INCH, HU_PER_INCH),
    }
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
                    push_runs(s, p);
                }
            }
            s.push_str(&format!("</{tag}>"));
        }
        s.push_str("</tr>");
    }
    s.push_str("</table>");
}

fn push_img(s: &mut String, img: &ImageData) {
    if img.bytes.is_empty() {
        return;
    }
    s.push_str("<img alt=\"");
    s.push_str(&escape_attr(img.alt_text.as_deref().unwrap_or("")));
    s.push_str("\" src=\"");
    s.push_str(&escape_attr(&image_src(img)));
    s.push_str("\"/>");
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
        assert!(html.contains("th{background:#ccfbf1"), "{html}");
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
    }

    #[test]
    fn read_honors_thead_rows() {
        let html = b"<table><thead><tr><td>Hop</td><td>File</td></tr></thead><tbody><tr><td>Input</td><td>letter.md</td></tr></tbody></table>";
        let doc = read(html).expect("html");
        let t = first_table(&doc);
        assert!(t.rows[0].header);
        assert!(!t.rows[1].header);
        assert!(t.header_row_count >= 1);
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
        assert!(html.contains("<ul><li>dock<ul><li>bay 4</li></ul></li></ul>"), "{html}");
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
        assert!(html.contains("sup{font-size:.75em"), "{html}");
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
        assert!(html.contains("sub{font-size:.75em"), "{html}");
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
        assert!(html.contains("mark{background:#fde68a"), "{html}");
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
        assert!(html.contains(r#"checked=""/>Replay the convert"#), "{html}");
        assert!(html.contains(r#"disabled=""/>Hope"#), "{html}");
        assert!(!html.contains("<p>Hope</p>"), "{html}");
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
    }

    #[test]
    fn thematic_breaks_emit_hr() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Break(BreakKind::Thematic));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains("<hr/>"), "{html}");
        assert!(html.contains("border-top:3px solid #134e4a"), "{html}");
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
        assert!(html.contains("font-family:ui-monospace"), "{html}");
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
        assert!(html.contains("<blockquote>Replay is evidence.</blockquote>"), "{html}");
        assert!(html.contains("border-left:4px solid #134e4a"), "{html}");
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
        assert!(html.contains("text-decoration:line-through"), "{html}");
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
        let tag = format!(r#"<img alt="mark" src="{src}"/>"#);
        assert!(html.contains(&tag), "{html}");
        assert!(html.contains("img{max-width:100%"), "{html}");
        let marker = r#"src="data:image/png;base64,"#;
        let start = html.find(marker).expect("data-uri img");
        let payload = &html[start + marker.len()..];
        let end = payload.find('"').expect("src end");
        assert!(end > 0, "empty data-uri is not a picture: {html}");
        let decoded = b64_decode(&payload[..end]).expect("b64");
        assert_eq!(decoded, img.bytes);
        assert_eq!(decoded, png);
        assert_eq!(decoded.as_slice(), include_bytes!("../../../docs/assets/mark.png"));
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
        assert!(html.contains("<td><strong>GPU-free</strong><em> byte-for-byte</em></td>"), "{html}");
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
        assert!(html.contains(r#"<img alt="mark" src="data:image/png;base64,"#), "{html}");
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
                Block::Paragraph(p) => p.runs.iter().any(|r| {
                    matches!(r.content, RunContent::Inline(InlineObject::Image(_)))
                }),
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
        assert!(out.contains(r#"<img alt="mark" src="data:image/png;base64,"#), "{out}");
        let start = out.find(r#"src="data:image/png;base64,"#).expect("src");
        let payload = &out[start + r#"src="data:image/png;base64,"#.len()..];
        let end = payload.find('"').expect("src end");
        let decoded = b64_decode(&payload[..end]).expect("b64");
        assert_eq!(decoded, png);
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
        let back = read(html.as_bytes()).expect("html");
        let p = first_para(&back);
        assert!(p.runs.iter().any(|r| matches!(
            &r.content,
            RunContent::Inline(InlineObject::Hyperlink { target, display })
                if target == "https://github.com/kevin9327/docagent" && display == "DocAgent"
        )));
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
        let back = read(html.as_bytes()).expect("html");
        let p = first_para(&back);
        assert!(p.runs.iter().any(|r| {
            r.style.highlight == Some(docagent_model::Color::MARK)
                && matches!(&r.content, RunContent::Text(t) if t == "hashes")
        }));
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
        section
            .body
            .push(Block::Paragraph(list_item("dock", NumberFormat::Bullet, 0, None)));
        section
            .body
            .push(Block::Paragraph(list_item("bay 4", NumberFormat::Bullet, 1, None)));
        section
            .body
            .push(Block::Paragraph(list_item("replay", NumberFormat::Bullet, 0, None)));
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
        assert!(html.contains("<ol><li>Hash the input</li><li>Run Convert</li></ol>"), "{html}");
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
    fn read_rejects_non_utf8() {
        assert!(matches!(read(&[0xFF, 0xFE, 0x00]), Err(Error::NotHtml)));
    }
}
