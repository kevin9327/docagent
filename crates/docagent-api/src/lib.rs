//! Sole engine API: Command / Query / Event. Every Command records a capsule.

#![forbid(unsafe_code)]

use docagent_capsule::{record, Capsule, Plan, ENGINE_VERSION};
use docagent_font::FontSet;
use docagent_html::to_html;
use docagent_layout::layout_document;
use docagent_model::{Diagnostic, Document};
use docagent_paint::paint;
use docagent_pdf::to_pdfa;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use ts_rs::TS;

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub enum InputKind {
    Docx,
    Odt,
    Markdown,
    Hwp5,
    Hwpx,
    Hwp3,
    Hml,
    IrJson,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, TS)]
pub enum ExportTarget {
    PdfA,
    Html,
    IrJson,
    Docx,
    Odt,
    Markdown,
    Hwp5,
    Hwpx,
    Hml,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub enum Command {
    Convert {
        input: Vec<u8>,
        input_kind: Option<InputKind>,
        targets: Vec<ExportTarget>,
    },
    Import {
        input: Vec<u8>,
        input_kind: Option<InputKind>,
    },
    Export {
        #[ts(type = "unknown")]
        document: Document,
        target: ExportTarget,
    },
    LayoutDocument {
        #[ts(type = "unknown")]
        document: Document,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub enum Query {
    EngineVersion,
    LastCapsule,
    LastDocument,
    CapsuleByInputHash {
        input_hash: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub enum Event {
    CommandCompleted {
        capsule_id: String,
    },
    Warning {
        message: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Converted {
    pub document: Document,
    pub pdf: Option<Vec<u8>>,
    pub html: Option<String>,
    pub ir_json: Option<String>,
    pub hwp5: Option<Vec<u8>>,
    pub hwpx: Option<Vec<u8>>,
    pub hml: Option<Vec<u8>>,
    pub docx: Option<Vec<u8>>,
    pub odt: Option<Vec<u8>>,
    pub markdown: Option<Vec<u8>>,
    pub capsule: Capsule,
    pub events: Vec<Event>,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("unknown input kind")]
    UnknownKind,
    #[error("hwp5: {0}")]
    Hwp5(String),
    #[error("hwpx: {0}")]
    Hwpx(String),
    #[error("hwp3: {0}")]
    Hwp3(String),
    #[error("hml: {0}")]
    Hml(String),
    #[error("docx: {0}")]
    Docx(String),
    #[error("odt: {0}")]
    Odt(String),
    #[error("markdown: {0}")]
    Markdown(String),
    #[error("pdf: {0}")]
    Pdf(String),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Default)]
pub struct Engine {
    last: Option<Converted>,
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn execute(&mut self, command: Command) -> Result<Converted, Error> {
        match command {
            Command::Convert {
                input,
                input_kind,
                targets,
            } => self.convert(input, input_kind, targets),
            Command::Import { input, input_kind } => {
                self.convert(input, input_kind, vec![ExportTarget::IrJson])
            }
            Command::Export { document, target } => {
                let bytes = serde_json::to_vec(&document)?;
                self.convert_document(bytes, document, vec![target])
            }
            Command::LayoutDocument { document } => {
                let bytes = serde_json::to_vec(&document)?;
                self.convert_document(bytes, document, vec![ExportTarget::IrJson])
            }
        }
    }

    pub fn query(&self, q: Query) -> serde_json::Value {
        match q {
            Query::EngineVersion => serde_json::json!({ "version": ENGINE_VERSION }),
            Query::LastCapsule => serde_json::to_value(self.last.as_ref().map(|c| &c.capsule))
                .unwrap_or(serde_json::Value::Null),
            Query::LastDocument => serde_json::to_value(self.last.as_ref().map(|c| &c.document))
                .unwrap_or(serde_json::Value::Null),
            Query::CapsuleByInputHash { input_hash } => {
                match &self.last {
                    Some(c) if c.capsule.input_hash == input_hash => {
                        serde_json::to_value(&c.capsule).unwrap_or(serde_json::Value::Null)
                    }
                    _ => serde_json::Value::Null,
                }
            }
        }
    }

    fn convert(
        &mut self,
        input: Vec<u8>,
        kind: Option<InputKind>,
        targets: Vec<ExportTarget>,
    ) -> Result<Converted, Error> {
        let kind = match kind {
            Some(k) => k,
            None => sniff(&input).ok_or(Error::UnknownKind)?,
        };
        let document = parse_kind(&input, kind)?;
        self.convert_document(input, document, targets)
    }

    fn convert_document(
        &mut self,
        input: Vec<u8>,
        mut document: Document,
        targets: Vec<ExportTarget>,
    ) -> Result<Converted, Error> {
        let fonts = FontSet::bundled();
        document
            .diagnostics
            .extend(docagent_font::resolve_for_document(&document, &fonts));
        let tree = layout_document(&document, &fonts);
        let list = paint(&tree);
        let mut out = Converted {
            document: document.clone(),
            pdf: None,
            html: None,
            ir_json: None,
            hwp5: None,
            hwpx: None,
            hml: None,
            docx: None,
            odt: None,
            markdown: None,
            capsule: Capsule {
                input_hash: String::new(),
                plan_hash: String::new(),
                output_hash: String::new(),
                engine_version: ENGINE_VERSION.into(),
                signature: None,
                verifying_key: None,
            },
            events: Vec::new(),
        };
        let mut output_chunks = Vec::new();
        for t in &targets {
            match t {
                ExportTarget::PdfA => {
                    let pdf = to_pdfa(&list, &fonts).map_err(Error::Pdf)?;
                    output_chunks.extend_from_slice(&pdf);
                    out.pdf = Some(pdf);
                }
                ExportTarget::Html => {
                    let html = to_html(&document);
                    output_chunks.extend_from_slice(html.as_bytes());
                    out.html = Some(html);
                }
                ExportTarget::IrJson => {
                    let json = serde_json::to_string_pretty(&document)?;
                    output_chunks.extend_from_slice(json.as_bytes());
                    out.ir_json = Some(json);
                }
                ExportTarget::Hwp5 => {
                    let b = docagent_hwp5::write(&document).map_err(|e| Error::Hwp5(e.to_string()))?;
                    output_chunks.extend_from_slice(&b);
                    out.hwp5 = Some(b);
                }
                ExportTarget::Hwpx => {
                    let b = docagent_hwpx::write(&document).map_err(|e| Error::Hwpx(e.to_string()))?;
                    output_chunks.extend_from_slice(&b);
                    out.hwpx = Some(b);
                }
                ExportTarget::Hml => {
                    let b = docagent_hml::write(&document).map_err(|e| Error::Hml(e.to_string()))?;
                    output_chunks.extend_from_slice(&b);
                    out.hml = Some(b);
                }
                ExportTarget::Docx => {
                    let (b, loss) = docagent_docx::hwp_to_docx_with_loss(&document)
                        .map_err(|e| Error::Docx(e.to_string()))?;
                    for d in loss {
                        out.events.push(Event::Warning {
                            message: d.message.clone(),
                        });
                        out.document.diagnostics.push(d);
                    }
                    output_chunks.extend_from_slice(&b);
                    out.docx = Some(b);
                }
                ExportTarget::Odt => {
                    let b = docagent_odt::write(&document).map_err(|e| Error::Odt(e.to_string()))?;
                    output_chunks.extend_from_slice(&b);
                    out.odt = Some(b);
                }
                ExportTarget::Markdown => {
                    let b = docagent_md::write(&document).map_err(|e| Error::Markdown(e.to_string()))?;
                    output_chunks.extend_from_slice(&b);
                    out.markdown = Some(b);
                }
            }
        }
        let plan = Plan {
            command: "Convert".into(),
            targets: targets.iter().map(|t| format!("{t:?}")).collect(),
            font_fingerprint: format!("{:x}", fonts.fingerprint),
            engine_version: ENGINE_VERSION.into(),
        };
        out.capsule = record(&input, &plan, &output_chunks, None);
        let capsule_id = out.capsule.input_hash.clone();
        out.events.push(Event::CommandCompleted { capsule_id });
        self.last = Some(out.clone());
        Ok(out)
    }
}

pub fn sniff(bytes: &[u8]) -> Option<InputKind> {
    if docagent_hwp3::sniff(bytes) {
        return Some(InputKind::Hwp3);
    }
    if docagent_hwp5::sniff(bytes) {
        return Some(InputKind::Hwp5);
    }
    if docagent_hwpx::sniff(bytes) {
        return Some(InputKind::Hwpx);
    }
    if docagent_docx::sniff(bytes) {
        return Some(InputKind::Docx);
    }
    if docagent_odt::sniff(bytes) {
        return Some(InputKind::Odt);
    }
    if docagent_hml::sniff(bytes) {
        return Some(InputKind::Hml);
    }
    if bytes.first() == Some(&b'{') {
        return Some(InputKind::IrJson);
    }
    if docagent_md::sniff(bytes) {
        return Some(InputKind::Markdown);
    }
    None
}

fn parse_kind(bytes: &[u8], kind: InputKind) -> Result<Document, Error> {
    match kind {
        InputKind::Hwp5 => docagent_hwp5::read(bytes).map_err(|e| Error::Hwp5(e.to_string())),
        InputKind::Hwpx => docagent_hwpx::read(bytes).map_err(|e| Error::Hwpx(e.to_string())),
        InputKind::Hwp3 => docagent_hwp3::read(bytes).map_err(|e| Error::Hwp3(e.to_string())),
        InputKind::Hml => docagent_hml::read(bytes).map_err(|e| Error::Hml(e.to_string())),
        InputKind::Docx => docagent_docx::read(bytes).map_err(|e| Error::Docx(e.to_string())),
        InputKind::Odt => docagent_odt::read(bytes).map_err(|e| Error::Odt(e.to_string())),
        InputKind::Markdown => docagent_md::read(bytes).map_err(|e| Error::Markdown(e.to_string())),
        InputKind::IrJson => Ok(serde_json::from_slice(bytes)?),
    }
}

pub fn ts_decls() -> String {
    format!(
        "{}\n{}\n{}\n{}\n{}",
        Command::decl(),
        Query::decl(),
        Event::decl(),
        ExportTarget::decl(),
        InputKind::decl()
    )
}

pub fn hwp_docx_loss(doc: &Document) -> Vec<Diagnostic> {
    match docagent_docx::hwp_to_docx_with_loss(doc) {
        Ok((_, d)) => d,
        Err(e) => vec![Diagnostic::parse_loss(e.to_string())],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use docagent_model::{Block, Paragraph, Section};

    fn sample_doc() -> Document {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("API sample")));
        doc.sections.push(section);
        doc
    }

    #[test]
    fn convert_records_three_hashes() {
        let bytes = docagent_hwp5::write(&sample_doc()).unwrap();
        let mut eng = Engine::new();
        let out = eng
            .execute(Command::Convert {
                input: bytes,
                input_kind: Some(InputKind::Hwp5),
                targets: vec![ExportTarget::IrJson, ExportTarget::Html, ExportTarget::PdfA],
            })
            .unwrap();
        assert_eq!(out.capsule.input_hash.len(), 64);
        assert_eq!(out.capsule.plan_hash.len(), 64);
        assert_eq!(out.capsule.output_hash.len(), 64);
        assert_ne!(out.capsule.input_hash, out.capsule.plan_hash);
        assert_ne!(out.capsule.plan_hash, out.capsule.output_hash);
        assert!(out.pdf.as_ref().unwrap().starts_with(b"%PDF"));
        assert!(out.html.as_ref().unwrap().contains("<article"));
        assert!(out.ir_json.as_ref().unwrap().contains("sections"));
    }

    #[test]
    fn convert_is_byte_identical_across_two_runs() {
        let bytes = docagent_hwp5::write(&sample_doc()).unwrap();
        let mut a = Engine::new();
        let mut b = Engine::new();
        let cmd = Command::Convert {
            input: bytes.clone(),
            input_kind: Some(InputKind::Hwp5),
            targets: vec![ExportTarget::PdfA, ExportTarget::Html, ExportTarget::IrJson],
        };
        let oa = a.execute(cmd.clone()).unwrap();
        let ob = b.execute(cmd).unwrap();
        assert_eq!(oa.pdf, ob.pdf);
        assert_eq!(oa.html, ob.html);
        assert_eq!(oa.ir_json, ob.ir_json);
        assert_eq!(oa.capsule.output_hash, ob.capsule.output_hash);
    }

    #[test]
    fn letter_md_is_byte_identical_across_two_runs() {
        let bytes = include_bytes!("../../../examples/letter.md");
        let mut a = Engine::new();
        let mut b = Engine::new();
        let cmd = Command::Convert {
            input: bytes.to_vec(),
            input_kind: None,
            targets: vec![ExportTarget::PdfA, ExportTarget::Html],
        };
        let oa = a.execute(cmd.clone()).unwrap();
        let ob = b.execute(cmd).unwrap();
        assert_eq!(oa.pdf.as_ref().unwrap(), ob.pdf.as_ref().unwrap());
        assert_eq!(oa.html.as_ref().unwrap(), ob.html.as_ref().unwrap());
        assert_eq!(oa.capsule, ob.capsule);
        assert_eq!(oa.capsule.input_hash.len(), 64);
        assert_ne!(oa.capsule.input_hash, oa.capsule.output_hash);
    }

    #[test]
    fn ts_contract_is_nonempty() {
        let d = ts_decls();
        assert!(d.contains("Command"));
        assert!(d.contains("Query"));
        assert!(d.contains("Event"));
    }
}
