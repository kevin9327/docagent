//! Host-callable WIT surface. The `.wit` contract lives at `wit/dochwp.wit`.

#![forbid(unsafe_code)]

use dochwp_api::{Command, Engine, ExportTarget, Query};

pub fn execute_json(command_json: &str) -> Result<String, String> {
    let command: Command = serde_json::from_str(command_json).map_err(|e| e.to_string())?;
    let mut engine = Engine::new();
    let out = engine.execute(command).map_err(|e| e.to_string())?;
    serde_json::to_string(&out.capsule).map_err(|e| e.to_string())
}

pub fn query_json(query_json: &str) -> Result<String, String> {
    let query: Query = serde_json::from_str(query_json).map_err(|e| e.to_string())?;
    let engine = Engine::new();
    Ok(engine.query(query).to_string())
}

pub fn default_convert_targets() -> Vec<ExportTarget> {
    vec![
        ExportTarget::PdfA,
        ExportTarget::Html,
        ExportTarget::IrJson,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wit_adapter_runs_query() {
        let r = query_json("{\"EngineVersion\":null}").or_else(|_| query_json("\"EngineVersion\""));
        assert!(r.is_ok() || r.err().is_some());
        let v = query_json("\"EngineVersion\"").expect("externally tagged query");
        assert!(v.contains("version"));
    }
}
