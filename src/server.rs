use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex};

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Option<Value>,
    result: Option<Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

struct Server {
    documents: Arc<Mutex<HashMap<String, String>>>,
}

impl Server {
    fn new() -> Self {
        Server {
            documents: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn handle_message(&self, request: JsonRpcRequest) -> Option<JsonRpcResponse> {
        match request.method.as_str() {
            "initialize" => self.handle_initialize(&request),
            "initialized" => {
                // No response needed
                None
            }
            "textDocument/didOpen" => {
                self.handle_did_open(&request);
                None
            }
            "textDocument/didChange" => {
                self.handle_did_change(&request);
                None
            }
            "textDocument/didClose" => {
                self.handle_did_close(&request);
                None
            }
            "textDocument/completion" => self.handle_completion(&request),
            "shutdown" => Some(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id,
                result: Some(Value::Null),
                error: None,
            }),
            _ => None,
        }
    }

    fn handle_initialize(&self, request: &JsonRpcRequest) -> Option<JsonRpcResponse> {
        let capabilities = json!({
            "textDocumentSync": {
                "openClose": true,
                "change": 1, // Incremental
            },
            "completionProvider": {
                "resolveProvider": false,
                "triggerCharacters": [":"]
            }
        });

        Some(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.clone(),
            result: Some(json!({
                "capabilities": capabilities,
                "serverInfo": {
                    "name": "emoji-language-server",
                    "version": "0.1.0"
                }
            })),
            error: None,
        })
    }

    fn handle_did_open(&self, request: &JsonRpcRequest) {
        if let Some(params) = &request.params {
            if let Some(text_document) = params.get("textDocument") {
                if let (Some(uri), Some(text)) = (
                    text_document.get("uri").and_then(|u| u.as_str()),
                    text_document.get("text").and_then(|t| t.as_str()),
                ) {
                    let mut docs = self.documents.lock().unwrap();
                    docs.insert(uri.to_string(), text.to_string());
                }
            }
        }
    }

    fn handle_did_change(&self, request: &JsonRpcRequest) {
        if let Some(params) = &request.params {
            if let Some(uri) = params
                .get("textDocument")
                .and_then(|td| td.get("uri").and_then(|u| u.as_str()))
            {
                if let Some(changes) = params.get("contentChanges").and_then(|c| c.as_array()) {
                    let mut docs = self.documents.lock().unwrap();
                    if let Some(change) = changes.first() {
                        if let Some(text) = change.get("text").and_then(|t| t.as_str()) {
                            docs.insert(uri.to_string(), text.to_string());
                        }
                    }
                }
            }
        }
    }

    fn handle_did_close(&self, request: &JsonRpcRequest) {
        if let Some(params) = &request.params {
            if let Some(uri) = params
                .get("textDocument")
                .and_then(|td| td.get("uri").and_then(|u| u.as_str()))
            {
                let mut docs = self.documents.lock().unwrap();
                docs.remove(uri);
            }
        }
    }

    fn handle_completion(&self, request: &JsonRpcRequest) -> Option<JsonRpcResponse> {
        let params = request.params.as_ref()?;
        let uri = params.get("textDocument")?.get("uri")?.as_str()?;
        let position = params.get("position")?;
        let line = position.get("line")?.as_u64()? as usize;
        let character = position.get("character")?.as_u64()? as usize;

        let docs = self.documents.lock().unwrap();
        let text = docs.get(uri)?;
        let lines: Vec<&str> = text.lines().collect();

        if line >= lines.len() {
            return Some(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id.clone(),
                result: Some(json!([])),
                error: None,
            });
        }

        let line_text = lines[line];
        if character > line_text.len() {
            return Some(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id.clone(),
                result: Some(json!([])),
                error: None,
            });
        }

        // Find the closest colon before the cursor
        let colon_pos = match line_text[..character].rfind(':') {
            Some(pos) => pos,
            None => {
                return Some(JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id.clone(),
                    result: Some(json!([])),
                    error: None,
                });
            }
        };

        let query = line_text[colon_pos + 1..character].to_lowercase();
        let start = colon_pos + 1;

        // Don't suggest if query is empty
        if query.is_empty() {
            return Some(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id.clone(),
                result: Some(json!([])),
                error: None,
            });
        }

        let mut completions = Vec::new();

        // Iterate through all emojis and filter
        for emoji in emojis::iter() {
            let emoji_char = emoji.as_str();
            let name = emoji.name();
            let shortcode = emoji.shortcode();

            let matches_name = name.to_lowercase().contains(&query);
            let matches_shortcode = shortcode
                .map(|code| code.to_lowercase().contains(&query))
                .unwrap_or(false);

            if matches_name || matches_shortcode {
                let label = if let Some(code) = shortcode {
                    format!(":{} {}", code, emoji_char)
                } else {
                    format!(":{} {}", name, emoji_char)
                };

                // Use both shortcode and name for filtering so it matches on either
                let filter_text = format!("{} {}", shortcode.unwrap_or(name), name);

                let completion_item = json!({
                    "label": label,
                    "kind": 1, // Text
                    "detail": name,
                    "insertText": emoji_char,
                    "filterText": filter_text,
                    "sortText": format!("{:06}", completions.len()),
                    "textEdit": {
                        "range": {
                            "start": {
                                "line": line,
                                "character": start - 1
                            },
                            "end": {
                                "line": line,
                                "character": character
                            }
                        },
                        "newText": emoji_char
                    }
                });

                completions.push(completion_item);

                if completions.len() >= 100 {
                    break;
                }
            }
        }

        Some(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.clone(),
            result: Some(Value::Array(completions)),
            error: None,
        })
    }
}

fn main() {
    let server = Arc::new(Server::new());
    let mut stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        let mut header_map = HashMap::new();

        // Read headers
        loop {
            let mut line = String::new();
            match stdin.read_line(&mut line) {
                Ok(0) => return, // EOF
                Ok(_) => {
                    if line == "\r\n" || line == "\n" {
                        break;
                    }
                    let parts: Vec<&str> = line.trim().split(": ").collect();
                    if parts.len() == 2 {
                        header_map.insert(parts[0].to_string(), parts[1].to_string());
                    }
                }
                Err(_) => return,
            }
        }

        let content_length: usize = match header_map.get("Content-Length") {
            Some(len_str) => match len_str.parse() {
                Ok(len) => len,
                Err(_) => continue,
            },
            None => continue,
        };

        let mut content = vec![0u8; content_length];
        if stdin.read_exact(&mut content).is_err() {
            return;
        }

        let content_str = String::from_utf8_lossy(&content);
        if let Ok(request) = serde_json::from_str::<JsonRpcRequest>(&content_str) {
            if let Some(response) = server.handle_message(request) {
                if let Ok(response_json) = serde_json::to_string(&response) {
                    let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                    let _ = stdout.write_all(header.as_bytes());
                    let _ = stdout.write_all(response_json.as_bytes());
                    let _ = stdout.flush();
                }
            }
        }
    }
}
