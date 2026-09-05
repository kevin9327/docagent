use docagent_conformance::{
    authored_table_fixtures, compare_fixture_set, published_weights, rhwp_root_guess,
};

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--weights") {
        println!("{}", published_weights());
        return Ok(());
    }
    let mut fixtures = authored_table_fixtures();
    let root = rhwp_root_guess();
    fixtures.extend(docagent_conformance::sibling_sample_fixtures(root.as_deref()));
    let board = compare_fixture_set(&fixtures, root);
    println!("{}", serde_json::to_string_pretty(&board).map_err(|e| e.to_string())?);
    if args.iter().any(|a| a == "--fail-on-drop" || a == "--require-win")
        && board.docagent_mean <= board.rhwp_mean
    {
        return Err(format!(
            "score drop or no win: docagent={} rhwp={}",
            board.docagent_mean, board.rhwp_mean
        ));
    }
    Ok(())
}
