//! `examples/letter.md` → DOCX must store PNG bytes under word/media
//! once the markdown contains `![`. Until then this test returns early.

use std::io::{Cursor, Read};
use std::path::Path;

use zip::ZipArchive;

const PNG_MAGIC: &[u8] = &[0x89, b'P', b'N', b'G'];

#[test]
fn letter_md_docx_stores_png_media() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    if !text.contains("![") {
        return;
    }

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    let docx = docagent_docx::write(&doc).expect("docx write");
    assert!(docagent_docx::sniff(&docx), "written bytes must sniff as DOCX");

    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut found_png = false;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).unwrap();
        let name = file.name().replace('\\', "/");
        if !name.starts_with("word/media/") || name.ends_with('/') {
            continue;
        }
        let mut stored = Vec::new();
        file.read_to_end(&mut stored).unwrap();
        if stored.starts_with(PNG_MAGIC) {
            found_png = true;
            break;
        }
    }
    assert!(
        found_png,
        "DOCX from letter.md must store PNG bytes under word/media"
    );
}
