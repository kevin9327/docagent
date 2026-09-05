//! `dochwpd` — localhost REST adapter over Command/Query.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

use dochwp_api::{Command, Engine, ExportTarget, Query};

fn main() -> Result<(), String> {
    let addr = std::env::var("DOCHWPD_BIND").unwrap_or_else(|_| "127.0.0.1:7090".into());
    let listener = TcpListener::bind(&addr).map_err(|e| e.to_string())?;
    eprintln!("dochwpd listening on {addr}");
    let mut engine = Engine::new();
    for incoming in listener.incoming() {
        let mut stream = incoming.map_err(|e| e.to_string())?;
        if let Err(e) = handle(&mut stream, &mut engine) {
            let body = format!("{{\"error\":\"{e}\"}}");
            let _ = write_http(&mut stream, 400, "application/json", body.as_bytes());
        }
    }
    Ok(())
}

fn handle(stream: &mut TcpStream, engine: &mut Engine) -> Result<(), String> {
    let mut buf = vec![0u8; 65536];
    let n = stream.read(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(n);
    let req = String::from_utf8_lossy(&buf);
    let first = req.lines().next().unwrap_or("");
    if first.starts_with("GET /version") {
        let v = engine.query(Query::EngineVersion).to_string();
        return write_http(stream, 200, "application/json", v.as_bytes());
    }
    if first.starts_with("POST /convert") {
        let body = req.split("\r\n\r\n").nth(1).unwrap_or("").as_bytes().to_vec();
        let out = engine
            .execute(Command::Convert {
                input: body,
                input_kind: None,
                targets: vec![ExportTarget::IrJson],
            })
            .map_err(|e| e.to_string())?;
        let json = serde_json::to_vec(&out.capsule).map_err(|e| e.to_string())?;
        return write_http(stream, 200, "application/json", &json);
    }
    write_http(stream, 404, "text/plain", b"not found")
}

fn write_http(stream: &mut TcpStream, code: u16, ctype: &str, body: &[u8]) -> Result<(), String> {
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        _ => "Not Found",
    };
    let header = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).map_err(|e| e.to_string())?;
    stream.write_all(body).map_err(|e| e.to_string())?;
    Ok(())
}
