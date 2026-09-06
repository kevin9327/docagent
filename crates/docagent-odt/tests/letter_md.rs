//! `examples/letter.md` → ODT must store image bytes under Pictures/
//! once the markdown contains `![`. Until then this test returns early.

use std::io::{Cursor, Read};
use std::path::Path;

use zip::ZipArchive;

#[test]
fn letter_md_odt_stores_picture_bytes() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/letter.md");
    let md = std::fs::read(&path).expect("examples/letter.md");
    let text = std::str::from_utf8(&md).expect("letter.md utf-8");
    if !text.contains("![") {
        return;
    }

    let doc = docagent_md::read(&md).expect("docagent-md read letter.md");
    let odt = docagent_odt::write(&doc).expect("odt write");
    assert!(docagent_odt::sniff(&odt), "written bytes must sniff as ODT");

    let mut zip = ZipArchive::new(Cursor::new(odt)).expect("odt zip");
    let mut names = Vec::new();
    for i in 0..zip.len() {
        let name = zip.by_index(i).unwrap().name().replace('\\', "/");
        if name.starts_with("Pictures/") && !name.ends_with('/') {
            names.push(name);
        }
    }
    assert!(
        !names.is_empty(),
        "ODT from letter.md must store an image under Pictures/"
    );
    for name in names {
        let mut stored = Vec::new();
        zip.by_name(&name)
            .unwrap()
            .read_to_end(&mut stored)
            .unwrap();
        assert!(!stored.is_empty(), "{name} must have bytes");
    }
}
