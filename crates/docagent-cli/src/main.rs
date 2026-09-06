use std::path::{Path, PathBuf};

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
    /// Convert twice. Exit 0 only if PDF/A, HTML, and capsule hashes match.
    Prove {
        input: PathBuf,
    },
    Version,
}

fn main() -> Result<(), String> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Prove { input } => {
            let bytes = load_input(&input)?;
            let cmd = Command::Convert {
                input: bytes,
                input_kind: None,
                targets: vec![ExportTarget::PdfA, ExportTarget::Html],
            };
            let mut a = Engine::new();
            let mut b = Engine::new();
            let oa = a.execute(cmd.clone()).map_err(|e| e.to_string())?;
            let ob = b.execute(cmd).map_err(|e| e.to_string())?;
            let pdf_eq = oa.pdf == ob.pdf;
            let html_eq = oa.html == ob.html;
            let cap_eq = oa.capsule == ob.capsule;
            let identical = pdf_eq && html_eq && cap_eq;
            let report = serde_json::json!({
                "identical": identical,
                "pdf_bytes_equal": pdf_eq,
                "html_bytes_equal": html_eq,
                "run1": oa.capsule,
                "run2": ob.capsule,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
            );
            if !identical {
                return Err("rerun produced different bytes".into());
            }
        }
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
            let bytes = load_input(&input)?;
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

fn load_input(path: &Path) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if !is_markdown(path) {
        return Ok(bytes);
    }
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return Ok(bytes);
    };
    Ok(rewrite_md_images(text, path).into_bytes())
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
}

fn rewrite_md_images(text: &str, md_path: &Path) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut in_fence = false;
    let mut at_line_start = true;
    while i < chars.len() {
        if at_line_start && starts_fence(&chars, i) {
            in_fence = !in_fence;
            while i < chars.len() {
                let c = chars[i];
                out.push(c);
                i += 1;
                if c == '\n' {
                    break;
                }
            }
            at_line_start = true;
            continue;
        }
        if in_fence {
            let c = chars[i];
            out.push(c);
            i += 1;
            at_line_start = c == '\n';
            continue;
        }
        if chars[i] == '!'
            && let Some((alt, src, title, next)) = parse_md_image(&chars, i)
            && let Some(uri) = local_data_uri(&src, md_path)
        {
            out.push_str("![");
            out.push_str(&alt);
            out.push_str("](");
            out.push_str(&uri);
            if let Some(title) = title {
                out.push_str(" \"");
                out.push_str(&title);
                out.push('"');
            }
            out.push(')');
            i = next;
            at_line_start = false;
            continue;
        }
        let c = chars[i];
        out.push(c);
        i += 1;
        at_line_start = c == '\n';
    }
    out
}

fn starts_fence(chars: &[char], i: usize) -> bool {
    let mut j = i;
    while j < chars.len() && (chars[j] == ' ' || chars[j] == '\t') {
        j += 1;
    }
    j + 2 < chars.len() && chars[j] == '`' && chars[j + 1] == '`' && chars[j + 2] == '`'
}

fn parse_md_image(
    chars: &[char],
    start: usize,
) -> Option<(String, String, Option<String>, usize)> {
    if start >= chars.len() || chars[start] != '!' {
        return None;
    }
    if start + 1 >= chars.len() || chars[start + 1] != '[' {
        return None;
    }
    let mut j = start + 2;
    while j < chars.len() && chars[j] != ']' {
        j += 1;
    }
    if j + 1 >= chars.len() || chars[j] != ']' || chars[j + 1] != '(' {
        return None;
    }
    let alt: String = chars[start + 2..j].iter().collect();
    let mut k = j + 2;
    while k < chars.len() && chars[k] != ')' {
        k += 1;
    }
    if k >= chars.len() {
        return None;
    }
    let dest: String = chars[j + 2..k].iter().collect();
    let (src, title) = split_src_title(&dest);
    if src.is_empty() {
        return None;
    }
    Some((alt, src, title, k + 1))
}

fn split_src_title(dest: &str) -> (String, Option<String>) {
    let dest = dest.trim();
    if dest.is_empty() {
        return (String::new(), None);
    }
    if let Some(rest) = dest.strip_prefix('<')
        && let Some(end) = rest.find('>')
    {
        let src = rest[..end].trim().to_string();
        let title = quoted_title(rest[end + 1..].trim());
        return (src, title);
    }
    let src_end = dest
        .char_indices()
        .find(|(_, c)| c.is_whitespace())
        .map(|(i, _)| i)
        .unwrap_or(dest.len());
    let src = dest[..src_end].to_string();
    (src, quoted_title(dest[src_end..].trim()))
}

fn quoted_title(rest: &str) -> Option<String> {
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

fn local_data_uri(src: &str, md_path: &Path) -> Option<String> {
    if src.starts_with("data:") || src.contains("://") || src.starts_with("//") {
        return None;
    }
    let path = resolve_image(src, md_path)?;
    let bytes = std::fs::read(path).ok()?;
    if bytes.is_empty() {
        return None;
    }
    let mut uri = String::from("data:");
    uri.push_str(mime_from_name(src));
    uri.push_str(";base64,");
    uri.push_str(&b64_encode(&bytes));
    Some(uri)
}

fn resolve_image(src: &str, md_path: &Path) -> Option<PathBuf> {
    let rel = Path::new(src);
    if rel.is_absolute() {
        return rel.is_file().then(|| rel.to_path_buf());
    }
    if let Ok(cwd) = std::env::current_dir() {
        let cand = cwd.join(rel);
        if cand.is_file() {
            return Some(cand);
        }
    }
    let mut dir = md_path.parent()?;
    loop {
        let cand = dir.join(rel);
        if cand.is_file() {
            return Some(cand);
        }
        dir = dir.parent()?;
    }
}

fn mime_from_name(name: &str) -> &'static str {
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
}

const B64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

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

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_letter() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md")
    }

    #[test]
    fn inlines_fixture_png_from_relative_src() {
        let md = "![Harbor mark](docs/assets/mark.png)\n";
        let out = rewrite_md_images(md, &repo_letter());
        assert!(out.contains("data:image/png;base64,"), "{out}");
        assert!(!out.contains("docs/assets/mark.png"), "{out}");
    }

    #[test]
    fn leaves_fence_and_missing_paths() {
        let md = "```\n![x](docs/assets/mark.png)\n```\n![no](missing.png)\n";
        let out = rewrite_md_images(md, &repo_letter());
        assert!(
            out.contains("```\n![x](docs/assets/mark.png)\n```"),
            "{out}"
        );
        assert!(out.contains("![no](missing.png)"), "{out}");
    }
}
