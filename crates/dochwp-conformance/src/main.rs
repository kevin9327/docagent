use dochwp_conformance::{compare_fixture_set, published_weights, rhwp_root_guess};
use dochwp_model::{Block, Document, Paragraph, Section};

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--weights") {
        println!("{}", published_weights());
        return Ok(());
    }
    let mut fixtures = Vec::new();
    for i in 0..8usize {
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Paragraph(Paragraph::from_text(
            format!("conformance fixture {i} 한글"),
        )));
        if i.is_multiple_of(2) {
            section.body.push(Block::Table(dochwp_model::Table::from_cells(vec![
                vec![format!("r{i}c0"), format!("r{i}c1")],
            ])));
        }
        doc.sections.push(section);
        let bytes = dochwp_hwp5::write(&doc).map_err(|e| e.to_string())?;
        fixtures.push((format!("fx{i}"), bytes, 1usize));
    }
    let board = compare_fixture_set(&fixtures, rhwp_root_guess());
    println!("{}", serde_json::to_string_pretty(&board).map_err(|e| e.to_string())?);
    if args.iter().any(|a| a == "--fail-on-drop" || a == "--require-win")
        && board.dochwp_mean <= board.rhwp_mean
    {
        return Err(format!(
            "score drop or no win: dochwp={} rhwp={}",
            board.dochwp_mean, board.rhwp_mean
        ));
    }
    Ok(())
}
