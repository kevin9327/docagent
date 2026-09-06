//! `examples/letter.md` → DOCX must include the Harbor mark as `a:blip`
//! in word/document.xml and PNG bytes under word/media.

use std::io::{Cursor, Read};
use std::path::Path;

use zip::ZipArchive;

const PNG_MAGIC: &[u8] = &[0x89, b'P', b'N', b'G'];

#[test]
fn letter_md_docx_includes_harbor_mark_as_blip() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    assert!(
        text.contains("!["),
        "examples/letter.md must contain a markdown image"
    );

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    let docx = docagent_docx::write(&doc).expect("docx write");
    assert!(docagent_docx::sniff(&docx), "written bytes must sniff as DOCX");

    let mut zip = ZipArchive::new(Cursor::new(docx)).expect("docx zip");
    let mut document_xml = String::new();
    {
        let mut file = zip
            .by_name("word/document.xml")
            .expect("word/document.xml");
        file.read_to_string(&mut document_xml).unwrap();
    }
    assert!(
        document_xml.contains("<a:blip r:embed="),
        "DOCX from letter.md must include the Harbor picture as a:blip in word/document.xml: {document_xml}"
    );

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
