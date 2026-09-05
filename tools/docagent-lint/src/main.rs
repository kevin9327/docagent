//! CI-enforced architecture lints.

use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("root");
    let mut errors = Vec::new();
    walk(&root.join("crates"), &mut |path, text| {
        lint_file(path, text, &mut errors);
    });
    if !errors.is_empty() {
        for e in &errors {
            eprintln!("{e}");
        }
        std::process::exit(1);
    }
}

fn walk(dir: &Path, f: &mut impl FnMut(&Path, &str)) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for ent in rd.flatten() {
        let p = ent.path();
        if p.is_dir() {
            walk(&p, f);
        } else if (p.extension().and_then(|s| s.to_str()) == Some("rs")
            || p.file_name().and_then(|s| s.to_str()) == Some("Cargo.toml"))
            && let Ok(text) = fs::read_to_string(&p)
        {
            f(&p, &text);
        }
    }
}

fn lint_file(path: &Path, text: &str, errors: &mut Vec<String>) {
    let rel = path.display().to_string();
    let is_layout = rel.replace('\\', "/").contains("/docagent-layout/");
    let is_codec = ["docagent-hwp5", "docagent-hwpx", "docagent-hwp3", "docagent-hml", "docagent-docx"]
        .iter()
        .any(|c| rel.replace('\\', "/").contains(&format!("/{c}/")));

    if is_layout && path.extension().and_then(|s| s.to_str()) == Some("rs") {
        for (i, line) in text.lines().enumerate() {
            if line.contains('f') && (line.contains("f32") || line.contains("f64")) {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                errors.push(format!("{rel}:{}: floating type in layout", i + 1));
            }
        }
    }

    if is_codec {
        for forbidden in [
            "docagent_layout",
            "docagent-layout",
            "docagent_paint",
            "docagent_raster",
            "docagent_pdf",
        ] {
            if text.contains(forbidden) {
                errors.push(format!("{rel}: codec imports {forbidden}"));
            }
        }
    }

    for sample in ["profile.hwp", "aift.hwp"] {
        if text.contains(sample) {
            errors.push(format!("{rel}: sample filename {sample}"));
        }
    }

    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if name.to_ascii_lowercase().contains("issue") && name.chars().any(|c| c.is_ascii_digit()) {
        errors.push(format!("{rel}: issue number in file name"));
    }
    for (i, line) in text.lines().enumerate() {
        let t = line.trim();
        if (t.starts_with("const ") || t.starts_with("fn test_") || t.starts_with("fn "))
            && t.to_ascii_lowercase().contains("issue")
            && t.chars().any(|c| c.is_ascii_digit())
        {
            errors.push(format!("{rel}:{}: issue number in identifier", i + 1));
        }
    }
}
