use std::sync::mpsc::Sender;

pub struct IngestEvent { pub agent: String, pub event: String, pub detail: Option<String> }

pub fn spawn_ingest(addr: &str, tx: Sender<IngestEvent>) -> std::io::Result<std::thread::JoinHandle<()>> {
    let server = tiny_http::Server::http(addr)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::AddrInUse, e.to_string()))?;
    Ok(std::thread::spawn(move || {
        for mut req in server.incoming_requests() {
            let url = req.url().to_string();
            if req.method() != &tiny_http::Method::Post || !url.starts_with("/event") {
                let _ = req.respond(tiny_http::Response::empty(404));
                continue;
            }
            let agent = url.split("agent=").nth(1)
                .map(|s| s.split('&').next().unwrap_or(s).to_string())
                .unwrap_or_default();
            let mut body = String::new();
            let _ = std::io::Read::read_to_string(&mut req.as_reader(), &mut body);
            let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
            let event = v.get("hook_event_name").or_else(|| v.get("type")).or_else(|| v.get("event"))
                .and_then(|x| x.as_str()).unwrap_or("").to_string();
            let detail = v.get("tool_name").and_then(|t| t.as_str()).map(|t| {
                let input = v.get("tool_input").map(|i| i.to_string()).unwrap_or_default();
                format!("{t} {input}")
            });
            if !agent.is_empty() && !event.is_empty() {
                let _ = tx.send(IngestEvent { agent, event, detail });
            }
            let _ = req.respond(tiny_http::Response::empty(204));
        }
    }))
}
