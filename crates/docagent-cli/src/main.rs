use std::path::PathBuf;

use clap::{Parser, Subcommand};
use docagent_api::{Command, Engine, ExportTarget};

#[derive(Parser)]
#[command(name = "docagent", version, about = "Document-only runtime for agents")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)] // clap flags; boxing breaks derive
enum Cmd {
    Convert {
        input: PathBuf,
        #[arg(long)]
        pdf: Option<PathBuf>,
        #[arg(long)]
        html: Option<PathBuf>,
        #[arg(long)]
        ir: Option<PathBuf>,
        #[arg(long)]
        capsule: Option<PathBuf>,
        #[arg(long)]
        docx: Option<PathBuf>,
        #[arg(long)]
        odt: Option<PathBuf>,
        #[arg(long)]
        md: Option<PathBuf>,
        #[arg(long)]
        hwp5: Option<PathBuf>,
        #[arg(long)]
        hwpx: Option<PathBuf>,
        #[arg(long)]
        hml: Option<PathBuf>,
    },
    Version,
}

fn main() -> Result<(), String> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
        }
        Cmd::Convert {
            input,
            pdf,
            html,
            ir,
            capsule,
            docx,
            odt,
            md,
            hwp5,
            hwpx,
            hml,
        } => {
            let bytes = std::fs::read(&input).map_err(|e| e.to_string())?;
            let mut targets = Vec::new();
            if pdf.is_some() {
                targets.push(ExportTarget::PdfA);
            }
            if html.is_some() {
                targets.push(ExportTarget::Html);
            }
            if ir.is_some() {
                targets.push(ExportTarget::IrJson);
            }
            if docx.is_some() {
                targets.push(ExportTarget::Docx);
            }
            if odt.is_some() {
                targets.push(ExportTarget::Odt);
            }
            if md.is_some() {
                targets.push(ExportTarget::Markdown);
            }
            if hwp5.is_some() {
                targets.push(ExportTarget::Hwp5);
            }
            if hwpx.is_some() {
                targets.push(ExportTarget::Hwpx);
            }
            if hml.is_some() {
                targets.push(ExportTarget::Hml);
            }
            if targets.is_empty() {
                targets.extend([
                    ExportTarget::PdfA,
                    ExportTarget::Html,
                    ExportTarget::IrJson,
                ]);
            }
            let mut engine = Engine::new();
            let out = engine
                .execute(Command::Convert {
                    input: bytes,
                    input_kind: None,
                    targets,
                })
                .map_err(|e| e.to_string())?;
            if let Some(path) = pdf {
                std::fs::write(path, out.pdf.as_ref().ok_or("pdf missing")?).map_err(|e| e.to_string())?;
            }
            if let Some(path) = html {
                std::fs::write(path, out.html.as_ref().ok_or("html missing")?).map_err(|e| e.to_string())?;
            }
            if let Some(path) = ir {
                std::fs::write(path, out.ir_json.as_ref().ok_or("ir missing")?).map_err(|e| e.to_string())?;
            }
            if let Some(path) = docx {
                std::fs::write(path, out.docx.as_ref().ok_or("docx missing")?).map_err(|e| e.to_string())?;
            }
            if let Some(path) = odt {
                std::fs::write(path, out.odt.as_ref().ok_or("odt missing")?).map_err(|e| e.to_string())?;
            }
            if let Some(path) = md {
                std::fs::write(path, out.markdown.as_ref().ok_or("markdown missing")?).map_err(|e| e.to_string())?;
            }
            if let Some(path) = hwp5 {
                std::fs::write(path, out.hwp5.as_ref().ok_or("hwp5 missing")?).map_err(|e| e.to_string())?;
            }
            if let Some(path) = hwpx {
                std::fs::write(path, out.hwpx.as_ref().ok_or("hwpx missing")?).map_err(|e| e.to_string())?;
            }
            if let Some(path) = hml {
                std::fs::write(path, out.hml.as_ref().ok_or("hml missing")?).map_err(|e| e.to_string())?;
            }
            let cap_json = serde_json::to_string_pretty(&out.capsule).map_err(|e| e.to_string())?;
            if let Some(path) = capsule {
                std::fs::write(path, &cap_json).map_err(|e| e.to_string())?;
            } else {
                println!("{cap_json}");
            }
        }
    }
    Ok(())
}
