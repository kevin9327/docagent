//! DOCX WordprocessingML codec (ECMA-376).
//!
//! Paragraph/run/table map onto `w:p` / `w:r` / `w:t` / `w:tbl`. Twips in the
//! package are converted with `twips_to_hu` / `hu_to_twips` (factor 5).

#![forbid(unsafe_code)]

use std::collections::{BTreeSet, HashMap};
use std::io::{Cursor, Read, Write};

use docagent_model::{
    Alignment, Block, BreakKind, CharStyle, Diagnostic, DiagnosticCode, Document, Float, Hu,
    ImageData, InlineObject, NumberFormat, NumberingRef, Paragraph, Run, RunContent, Section,
    Severity, Table, TableCell, TableRow, Underline, WrapMode, DEFAULT_FONT_SIZE_HU, HU_PER_INCH,
    hu_to_twips, twips_to_hu,
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

fn hu_to_emu(hu: Hu) -> i64 {
    i64::from(hu.max(1)).saturating_mul(EMU_PER_HU)
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
        zip.start_file("word/numbering.xml", deflated)?;
        zip.write_all(NUMBERING_XML.as_bytes())?;
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
        r#"<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/></Types>"#,
    );
    s
}

fn jpeg_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8, 0xFF])
}

fn is_jpeg(img: &ImageData) -> bool {
    img.mime.eq_ignore_ascii_case("image/jpeg")
        || img.mime.eq_ignore_ascii_case("image/jpg")
        || jpeg_magic(&img.bytes)
}

fn ext_from_image(img: &ImageData) -> &'static str {
    if is_jpeg(img) {
        "jpeg"
    } else {
        ext_from_mime(&img.mime)
    }
}

fn content_type_for_image(img: &ImageData) -> &str {
    if is_jpeg(img) {
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
                    if let RunContent::Inline(InlineObject::Hyperlink { target, .. }) = &run.content {
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
            for block in &section.body {
                match block {
                    Block::Paragraph(p) => s.push_str(&p_xml(p, &mut ctx)),
                    Block::Table(t) => s.push_str(&tbl_xml(t, &mut ctx)),
                    Block::Break(BreakKind::Thematic) => s.push_str(
                        r#"<w:p><w:pPr><w:pStyle w:val="HorizontalLine"/><w:pBdr><w:bottom w:val="single" w:sz="12" w:space="1" w:color="134E4A"/></w:pBdr></w:pPr></w:p>"#,
                    ),
                    Block::Float(Float::Image(img)) => {
                        let rid = ctx.next_image_rid();
                        s.push_str("<w:p>");
                        s.push_str(&drawing_run_xml(img, rid));
                        s.push_str("</w:p>");
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
        s.push_str(&format!(
            r#"<w:pgMar w:top="{}" w:right="{}" w:bottom="{}" w:left="{}" w:header="{}" w:footer="{}" w:gutter="{}"/>"#,
            hu_to_twips(section.page.margin_top),
            hu_to_twips(section.page.margin_right),
            hu_to_twips(section.page.margin_bottom),
            hu_to_twips(section.page.margin_left),
            hu_to_twips(section.page.header_distance),
            hu_to_twips(section.page.footer_distance),
            hu_to_twips(section.page.gutter)
        ));
        s.push_str("</w:sectPr>");
        let _ = si;
    }
    s.push_str("</w:body></w:document>");
    s
}

const STYLES_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/><w:pPr><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:sz w:val="36"/><w:szCs w:val="36"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/><w:pPr><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:b/><w:sz w:val="28"/><w:szCs w:val="28"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Heading3"><w:name w:val="heading 3"/><w:basedOn w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/><w:pPr><w:outlineLvl w:val="2"/></w:pPr><w:rPr><w:b/><w:sz w:val="24"/><w:szCs w:val="24"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr><w:pBdr><w:left w:val="single" w:sz="24" w:space="4" w:color="134E4A"/></w:pBdr><w:ind w:left="144"/></w:pPr></w:style><w:style w:type="character" w:styleId="Code"><w:name w:val="Code"/><w:rPr><w:rFonts w:ascii="Consolas" w:hAnsi="Consolas"/><w:shd w:val="clear" w:fill="CCFBF1"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="HorizontalLine"><w:name w:val="Horizontal Line"/><w:basedOn w:val="Normal"/><w:pPr><w:pBdr><w:bottom w:val="single" w:sz="12" w:space="1" w:color="134E4A"/></w:pBdr></w:pPr></w:style><w:style w:type="paragraph" w:styleId="CodeBlock"><w:name w:val="Code Block"/><w:basedOn w:val="Normal"/><w:pPr><w:shd w:val="clear" w:fill="CCFBF1"/></w:pPr><w:rPr><w:rFonts w:ascii="Consolas" w:hAnsi="Consolas"/></w:rPr></w:style></w:styles>"#;

fn hu_to_half_points(hu: i32) -> i32 {
    (hu / 50).max(1)
}

fn half_points_to_hu(hp: i32) -> i32 {
    hp.saturating_mul(50)
}

const NUMBERING_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:abstractNum w:abstractNumId="0"><w:multiLevelType w:val="hybridMultilevel"/><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="•"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl><w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="•"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="1440" w:hanging="360"/></w:pPr></w:lvl><w:lvl w:ilvl="2"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="•"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="2160" w:hanging="360"/></w:pPr></w:lvl></w:abstractNum><w:abstractNum w:abstractNumId="1"><w:multiLevelType w:val="hybridMultilevel"/><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl><w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%2."/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="1440" w:hanging="360"/></w:pPr></w:lvl><w:lvl w:ilvl="2"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%3."/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="2160" w:hanging="360"/></w:pPr></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num><w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num></w:numbering>"#;

fn drawing_run_xml(img: &ImageData, rid: u32) -> String {
    let cx = hu_to_emu(img.width);
    let cy = hu_to_emu(img.height);
    let name = format!("Picture {rid}");
    let descr = img
        .alt_text
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| format!(r#" descr="{}""#, xml_escape(s)))
        .unwrap_or_default();
    let wrap_body = format!(
        r#"<wp:extent cx="{cx}" cy="{cy}"/><wp:effectExtent l="0" t="0" r="0" b="0"/><wp:docPr id="{rid}" name="{name}"{descr}/><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/></wp:cNvGraphicFramePr><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="0" name="image{rid}"{descr}/><pic:cNvPicPr><a:picLocks noChangeAspect="1"/></pic:cNvPicPr></pic:nvPicPr><pic:blipFill><a:blip r:embed="rId{rid}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic>"#
    );
    let framed = match img.wrap {
        WrapMode::Inline => format!(
            r#"<wp:inline distT="0" distB="0" distL="0" distR="0">{wrap_body}</wp:inline>"#
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
                r#"<wp:anchor distT="0" distB="0" distL="0" distR="0" simplePos="0" relativeHeight="0" behindDoc="{behind}" locked="0" layoutInCell="1" allowOverlap="1"><wp:simplePos x="0" y="0"/><wp:positionH relativeFrom="column"><wp:posOffset>0</wp:posOffset></wp:positionH><wp:positionV relativeFrom="paragraph"><wp:posOffset>0</wp:posOffset></wp:positionV>{wrap_body}{wrap_el}</wp:anchor>"#
            )
        }
    };
    format!(r#"<w:r><w:drawing>{framed}</w:drawing></w:r>"#)
}

fn p_xml(p: &Paragraph, ctx: &mut WriteCtx) -> String {
    let mut s = String::from("<w:p>");
    let heading = p.outline_level.filter(|l| *l > 0);
    let num = p.numbering.as_ref();
    if heading.is_some() || num.is_some() || p.quote || p.code_block {
        s.push_str("<w:pPr>");
        if let Some(level) = heading {
            let outline = level.saturating_sub(1);
            s.push_str(&format!(
                r#"<w:pStyle w:val="Heading{level}"/><w:outlineLvl w:val="{outline}"/>"#
            ));
        }
        if p.quote {
            s.push_str(
                r#"<w:pStyle w:val="Quote"/><w:pBdr><w:left w:val="single" w:sz="24" w:space="4" w:color="134E4A"/></w:pBdr>"#,
            );
        }
        if p.code_block {
            s.push_str(
                r#"<w:pStyle w:val="CodeBlock"/><w:shd w:val="clear" w:fill="CCFBF1"/>"#,
            );
        }
        if let Some(n) = num {
            let num_id = match n.format {
                Some(NumberFormat::Decimal) => 2,
                _ => 1,
            };
            let ilvl = n.level;
            s.push_str(&format!(
                r#"<w:numPr><w:ilvl w:val="{ilvl}"/><w:numId w:val="{num_id}"/></w:numPr>"#
            ));
        }
        s.push_str("</w:pPr>");
    }
    let task_prefix = match p.numbering.as_ref() {
        Some(n) if n.format == Some(NumberFormat::Task) => {
            if n.start == Some(1) {
                Some("[x] ")
            } else {
                Some("[ ] ")
            }
        }
        _ => None,
    };
    let mut prefixed = task_prefix.is_none();
    let mut wrote = false;
    for run in &p.runs {
        match &run.content {
            RunContent::Inline(InlineObject::Hyperlink { display, .. }) => {
                if display.is_empty() {
                    continue;
                }
                let rid = ctx.next_link_rid();
                s.push_str(&format!(r#"<w:hyperlink r:id="rId{rid}">"#));
                s.push_str(r#"<w:r><w:rPr><w:u w:val="single"/><w:color w:val="0D9488"/></w:rPr><w:t xml:space="preserve">"#);
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
                if run.style.bold
                    || run.style.italic
                    || run.style.strike
                    || run.style.code
                    || run.style.highlight.is_some()
                    || run.style.underline != Underline::None
                    || run.style.superscript
                    || run.style.subscript
                    || sz != default_sz
                {
                    s.push_str("<w:rPr>");
                    if run.style.bold {
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
                    if run.style.code {
                        s.push_str(
                            r#"<w:rStyle w:val="Code"/><w:shd w:val="clear" w:fill="CCFBF1"/>"#,
                        );
                    }
                    if run.style.highlight.is_some() {
                        s.push_str(r#"<w:highlight w:val="yellow"/>"#);
                    }
                    if sz != default_sz {
                        s.push_str(&format!(r#"<w:sz w:val="{sz}"/><w:szCs w:val="{sz}"/>"#));
                    }
                    s.push_str("</w:rPr>");
                }
                s.push_str("<w:t xml:space=\"preserve\">");
                if !prefixed && let Some(pre) = task_prefix {
                    s.push_str(pre);
                    prefixed = true;
                }
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

fn tbl_xml(table: &Table, ctx: &mut WriteCtx) -> String {
    let mut s = String::from("<w:tbl><w:tblPr/><w:tblGrid>");
    if let Some(row) = table.rows.first() {
        for cell in &row.cells {
            s.push_str(&format!(
                r#"<w:gridCol w:w="{}"/>"#,
                hu_to_twips(cell.width)
            ));
        }
    }
    s.push_str("</w:tblGrid>");
    for row in &table.rows {
        s.push_str("<w:tr>");
        if row.header {
            s.push_str(r#"<w:trPr><w:tblHeader/></w:trPr>"#);
        }
        for cell in &row.cells {
            let shd = if row.header {
                r#"<w:shd w:val="clear" w:fill="CCFBF1"/>"#
            } else {
                ""
            };
            s.push_str(&format!(
                r#"<w:tc><w:tcPr><w:tcW w:w="{}" w:type="dxa"/>{shd}</w:tcPr>"#,
                hu_to_twips(cell.width)
            ));
            let mut wrote_p = false;
            for b in &cell.blocks {
                if let Block::Paragraph(p) = b {
                    s.push_str(&p_xml(p, ctx));
                    wrote_p = true;
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
                        if attr(&e, "val").as_deref() == Some("Code") {
                            run_code = true;
                        }
                    }
                    "shd" if in_rpr => {
                        if attr(&e, "fill").as_deref() == Some("CCFBF1") {
                            run_code = true;
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
                            apply_task_marker(&mut p);
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
                            apply_task_marker(&mut p);
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
                            apply_task_marker(&mut p);
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
        apply_task_marker(&mut p);
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

fn apply_task_marker(p: &mut Paragraph) {
    let Some(run) = p.runs.first_mut() else {
        return;
    };
    let RunContent::Text(text) = &mut run.content else {
        return;
    };
    let checked = text.starts_with("[x] ") || text.starts_with("[X] ");
    let unchecked = text.starts_with("[ ] ");
    if !checked && !unchecked {
        return;
    }
    *text = text[4..].to_string();
    let level = p.numbering.as_ref().map(|n| n.level).unwrap_or(0);
    p.numbering = Some(NumberingRef {
        definition_id: 2,
        level,
        start: checked.then_some(1),
        format: Some(NumberFormat::Task),
    });
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
    if let Some(id) = num_id.take() {
        let format = if id == 2 {
            NumberFormat::Decimal
        } else {
            NumberFormat::Bullet
        };
        let start = if format == NumberFormat::Decimal && *ilvl == 0 {
            let n = *decimal_seq;
            *decimal_seq = decimal_seq.saturating_add(1);
            Some(n)
        } else {
            None
        };
        if format == NumberFormat::Bullet && *ilvl == 0 {
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
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/assets/mark.png");
        std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
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
        assert!(xml.contains(r#"w:val="Quote""#), "{xml}");
        assert!(xml.contains("w:pBdr"), "{xml}");
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.quote);
        assert!(p.plain_text().contains("Replay is evidence"));
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
        assert!(xml.contains(r#"w:val="superscript""#), "{xml}");
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.superscript && matches!(&r.content, RunContent::Text(t) if t == "A")
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
        assert!(xml.contains(r#"w:val="subscript""#), "{xml}");
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.subscript && matches!(&r.content, RunContent::Text(t) if t == "2")
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
        assert!(xml.contains(r#"w:val="single""#), "{xml}");
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
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.highlight.is_some() && matches!(&r.content, RunContent::Text(t) if t == "hashes")
        }));
    }

    #[test]
    fn roundtrip_keeps_task_item() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Replay the convert");
        p.numbering = Some(NumberingRef {
            definition_id: 2,
            level: 0,
            start: Some(1),
            format: Some(NumberFormat::Task),
        });
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = document_xml(&doc);
        assert!(xml.contains("[x] "), "{xml}");
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
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.code_block);
        assert!(p.plain_text().contains("docagent prove"));
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
        assert!(xml.contains(r#"w:fill="CCFBF1""#), "{xml}");
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
        assert!(xml.contains("<w:strike/>"), "docx must emit w:strike: {xml}");
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
        assert!(xml.contains("w:u"), "{xml}");
        assert!(xml.contains(r#"w:val="0D9488""#), "{xml}");
        let rels = document_rels(&doc);
        assert!(rels.contains("hyperlink"), "{rels}");
        assert!(rels.contains("https://github.com/kevin9327/docagent"), "{rels}");
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("para");
        };
        assert!(p.runs.iter().any(|r| matches!(
            &r.content,
            RunContent::Inline(InlineObject::Hyperlink { target, display })
                if target == "https://github.com/kevin9327/docagent" && display == "DocAgent"
        )));
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
        assert!(xml.contains(r#"w:sz w:val="36""#), "{xml}");
        let bytes = write(&doc).unwrap();
        assert!(sniff(&bytes));
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("h1");
        };
        assert_eq!(p.outline_level, Some(1));
        assert!(p.runs[0].style.bold);
        assert_eq!(p.runs[0].style.size, 1800);
        let Block::Paragraph(p2) = &back.sections[0].body[1] else {
            panic!("h2");
        };
        assert_eq!(p2.outline_level, Some(2));
        assert_eq!(p2.runs[0].style.size, 1400);
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
        assert!(xml.contains(r#"w:fill="CCFBF1""#), "{xml}");
        let back = roundtrip(&doc).unwrap();
        let Block::Table(t) = &back.sections[0].body[0] else {
            panic!("table");
        };
        assert!(t.rows[0].header);
        assert!(!t.rows[1].header);
        assert_eq!(t.header_row_count, 1);
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
        assert!(xml.contains(r#"cx="914400""#), "{xml}");
        assert!(xml.contains(r#"cy="457200""#), "{xml}");
        assert!(xml.contains(r#"descr="DocAgent mark""#), "{xml}");

        let rels = document_rels(&doc);
        assert!(rels.contains("/relationships/image"), "{rels}");
        assert!(rels.contains("media/image1.png"), "{rels}");

        let bytes = write(&doc).unwrap();
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
