use std::sync::mpsc::Sender;

pub struct IngestEvent { pub agent: String, pub event: String, pub detail: Option<String> }

/// Percent-decode a query-string component: %XX -> byte, '+' -> space.
/// Invalid escapes are passed through unchanged.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len()
                && bytes[i + 1].is_ascii_hexdigit()
                && bytes[i + 2].is_ascii_hexdigit() =>
            {
                let hi = (bytes[i + 1] as char).to_digit(16).unwrap();
                let lo = (bytes[i + 2] as char).to_digit(16).unwrap();
                out.push(((hi << 4) | lo) as u8);
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Extract the value of the `agent` query parameter, percent-decoded.
fn parse_agent_param(query: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let mut parts = pair.splitn(2, '=');
        let key = parts.next().unwrap_or("");
        let value = parts.next().unwrap_or("");
        if key == "agent" {
            Some(percent_decode(value))
        } else {
            None
        }
    })
}

pub fn spawn_ingest(addr: &str, tx: Sender<IngestEvent>) -> std::io::Result<std::thread::JoinHandle<()>> {
    let server = tiny_http::Server::http(addr)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::AddrInUse, e.to_string()))?;
    Ok(std::thread::spawn(move || {
        for mut req in server.incoming_requests() {
            let url = req.url().to_string();
            let (path, query) = match url.split_once('?') {
                Some((p, q)) => (p, q),
                None => (url.as_str(), ""),
            };
            if req.method() != &tiny_http::Method::Post || path != "/event" {
                let _ = req.respond(tiny_http::Response::empty(404));
                continue;
            }
            let agent = parse_agent_param(query).unwrap_or_default();
            let mut body = String::new();
            let _ = std::io::Read::read_to_string(&mut req.as_reader(), &mut body);
            let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
            let event = v.get("hook_event_name").or_else(|| v.get("type")).or_else(|| v.get("event"))
                .and_then(|x| x.as_str()).unwrap_or("").to_string();
            let detail = v.get("tool_name").and_then(|t| t.as_str()).map(|t| {
                match v.get("tool_input") {
                    Some(input) => format!("{t} {input}"),
                    None => t.to_string(),
                }
            });
            if !agent.is_empty() && !event.is_empty() {
                let _ = tx.send(IngestEvent { agent, event, detail });
            }
            let _ = req.respond(tiny_http::Response::empty(204));
        }
    }))
}
