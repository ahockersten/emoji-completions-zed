use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex};

// Convert UTF-16 code unit offset to UTF-8 byte offset
fn utf16_offset_to_utf8(s: &str, utf16_offset: usize) -> usize {
    let mut byte_pos = 0;
    let mut utf16_count = 0;

    for ch in s.chars() {
        if utf16_count >= utf16_offset {
            break;
        }
        // Each char contributes 1 or 2 UTF-16 code units
        let utf16_units = if ch as u32 > 0xFFFF { 2 } else { 1 };
        utf16_count += utf16_units;
        byte_pos += ch.len_utf8();
    }

    byte_pos
}

// Convert UTF-8 byte offset to UTF-16 code unit offset
fn utf8_offset_to_utf16(s: &str, byte_offset: usize) -> usize {
    let mut utf16_count = 0;
    let mut current_byte = 0;

    for ch in s.chars() {
        if current_byte >= byte_offset {
            break;
        }
        current_byte += ch.len_utf8();
        let utf16_units = if ch as u32 > 0xFFFF { 2 } else { 1 };
        utf16_count += utf16_units;
    }

    utf16_count
}

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
                            if let Some(range) = change.get("range") {
                                // Incremental change - apply the text to the specified range
                                if let Some(doc) = docs.get_mut(uri) {
                                    if let (Some(start), Some(end)) =
                                        (range.get("start"), range.get("end"))
                                    {
                                        if let (
                                            Some(start_line),
                                            Some(start_char),
                                            Some(end_line),
                                            Some(end_char),
                                        ) = (
                                            start.get("line").and_then(|l| l.as_u64()),
                                            start.get("character").and_then(|c| c.as_u64()),
                                            end.get("line").and_then(|l| l.as_u64()),
                                            end.get("character").and_then(|c| c.as_u64()),
                                        ) {
                                            let lines: Vec<&str> = doc.lines().collect();
                                            let start_line_idx = start_line as usize;
                                            let end_line_idx = end_line as usize;

                                            if start_line_idx < lines.len()
                                                && end_line_idx < lines.len()
                                            {
                                                let mut new_doc = String::new();

                                                // Add lines before the changed range
                                                for i in 0..start_line_idx {
                                                    new_doc.push_str(lines[i]);
                                                    new_doc.push('\n');
                                                }

                                                // Build the modified line(s)
                                                if start_line_idx == end_line_idx {
                                                    // Single line change
                                                    let line = lines[start_line_idx];
                                                    let start_byte = utf16_offset_to_utf8(
                                                        line,
                                                        start_char as usize,
                                                    );
                                                    let end_byte = utf16_offset_to_utf8(
                                                        line,
                                                        end_char as usize,
                                                    );
                                                    new_doc.push_str(&line[..start_byte]);
                                                    new_doc.push_str(text);
                                                    new_doc.push_str(&line[end_byte..]);
                                                } else {
                                                    // Multi-line change
                                                    let start_line_text = lines[start_line_idx];
                                                    let start_byte = utf16_offset_to_utf8(
                                                        start_line_text,
                                                        start_char as usize,
                                                    );
                                                    new_doc
                                                        .push_str(&start_line_text[..start_byte]);
                                                    new_doc.push_str(text);

                                                    let end_line_text = lines[end_line_idx];
                                                    let end_byte = utf16_offset_to_utf8(
                                                        end_line_text,
                                                        end_char as usize,
                                                    );
                                                    new_doc.push_str(&end_line_text[end_byte..]);
                                                }

                                                // Add lines after the changed range
                                                for i in (end_line_idx + 1)..lines.len() {
                                                    new_doc.push('\n');
                                                    new_doc.push_str(lines[i]);
                                                }

                                                docs.insert(uri.to_string(), new_doc);
                                            }
                                        }
                                    }
                                }
                            } else {
                                // Full document sync (no range provided)
                                docs.insert(uri.to_string(), text.to_string());
                            }
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

        // Convert UTF-16 code unit offset to UTF-8 byte offset
        let byte_pos = utf16_offset_to_utf8(line_text, character);

        if byte_pos > line_text.len() {
            return Some(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id.clone(),
                result: Some(json!([])),
                error: None,
            });
        }

        // Find the closest colon before the cursor
        let colon_pos = match line_text[..byte_pos].rfind(':') {
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

        let query = line_text[colon_pos + 1..byte_pos].to_lowercase();

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

                // Convert byte positions back to UTF-16 for the textEdit range
                let colon_utf16 = utf8_offset_to_utf16(line_text, colon_pos);

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
                                "character": colon_utf16
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
