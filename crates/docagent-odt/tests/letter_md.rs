//! `examples/letter.md` → ODT must emit `draw:image` in content.xml
//! and store Harbor mark bytes under Pictures/.

use std::io::{Cursor, Read};
use std::path::Path;

use zip::ZipArchive;

const PNG_MAGIC: &[u8] = &[0x89, b'P', b'N', b'G'];

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
    let mut content = String::new();
    zip.by_name("content.xml")
        .expect("content.xml")
        .read_to_string(&mut content)
        .unwrap();
    assert!(
        content.contains("<draw:image"),
        "ODT from letter.md must include draw:image: {content}"
    );
    assert!(
        content.contains("Pictures/"),
        "draw:image must href Pictures/: {content}"
    );
    assert!(
        content.contains("Harbor mark"),
        "Harbor mark alt must appear on the frame: {content}"
    );

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
        assert!(
            stored.starts_with(PNG_MAGIC),
            "{name} must be the Harbor PNG"
        );
    }
}
