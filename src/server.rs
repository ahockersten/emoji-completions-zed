use std::collections::HashMap;
use std::error::Error;

use line_index::{LineIndex, WideEncoding};
use lsp_server::{Connection, Message, Response};
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse,
    CompletionTextEdit, DidChangeTextDocumentParams, DidOpenTextDocumentParams, Position, Range,
    ServerCapabilities, TextDocumentSyncKind, TextEdit,
};

fn main() -> Result<(), Box<dyn Error + Sync + Send>> {
    let (connection, io_threads) = Connection::stdio();

    let server_capabilities = serde_json::to_value(&ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncKind::FULL.into()),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![":".to_string()]),
            resolve_provider: Some(false),
            ..Default::default()
        }),
        ..Default::default()
    })
    .unwrap();

    let _initialization_params = connection.initialize(server_capabilities)?;
    main_loop(connection)?;
    io_threads.join()?;
    Ok(())
}

fn main_loop(connection: Connection) -> Result<(), Box<dyn Error + Sync + Send>> {
    let mut documents: HashMap<String, String> = HashMap::new();

    for msg in connection.receiver.iter() {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    return Ok(());
                }
                match req.method.as_str() {
                    "textDocument/completion" => {
                        let (id, params) =
                            req.extract::<CompletionParams>("textDocument/completion")?;
                        let result = handle_completion(&documents, params);
                        let result = serde_json::to_value(&result).unwrap();
                        let resp = Response {
                            id,
                            result: Some(result),
                            error: None,
                        };
                        connection.sender.send(Message::Response(resp))?;
                    }
                    _ => {
                        // ignore other requests
                    }
                }
            }
            Message::Response(_) => {}
            Message::Notification(not) => match not.method.as_str() {
                "textDocument/didOpen" => {
                    let params =
                        not.extract::<DidOpenTextDocumentParams>("textDocument/didOpen")?;
                    documents.insert(
                        params.text_document.uri.to_string(),
                        params.text_document.text,
                    );
                }
                "textDocument/didChange" => {
                    let params =
                        not.extract::<DidChangeTextDocumentParams>("textDocument/didChange")?;
                    if let Some(change) = params.content_changes.into_iter().next() {
                        documents.insert(params.text_document.uri.to_string(), change.text);
                    }
                }
                _ => {}
            },
        }
    }
    Ok(())
}

fn handle_completion(
    documents: &HashMap<String, String>,
    params: CompletionParams,
) -> Option<CompletionResponse> {
    let uri = params.text_document_position.text_document.uri.to_string();
    let text = documents.get(&uri)?;
    let position = params.text_document_position.position;
    let line_idx = position.line as usize;

    let lines: Vec<&str> = text.lines().collect();
    if line_idx >= lines.len() {
        return Some(CompletionResponse::Array(vec![]));
    }
    let line_text = lines[line_idx];

    // Use line-index to convert UTF-16 offset to UTF-8 byte offset
    let line_index = LineIndex::new(line_text);
    let byte_offset = match line_index.to_utf8(
        WideEncoding::Utf16,
        line_index::WideLineCol {
            line: 0,
            col: position.character,
        },
    ) {
        Some(line_col) => line_col.col as usize,
        None => return Some(CompletionResponse::Array(vec![])),
    };

    if byte_offset > line_text.len() {
        return Some(CompletionResponse::Array(vec![]));
    }

    // Find the closest colon before the cursor
    let colon_pos = match line_text[..byte_offset].rfind(':') {
        Some(pos) => pos,
        None => return Some(CompletionResponse::Array(vec![])),
    };

    let query = line_text[colon_pos + 1..byte_offset].to_lowercase();
    if query.is_empty() {
        return Some(CompletionResponse::Array(vec![]));
    }

    let mut completions = Vec::new();

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

            let filter_text = format!("{} {}", shortcode.unwrap_or(name), name);

            // Convert UTF-8 byte offset back to UTF-16 for LSP
            let colon_utf16 = match line_index.to_wide(
                WideEncoding::Utf16,
                line_index::LineCol {
                    line: 0,
                    col: colon_pos as u32,
                },
            ) {
                Some(wide_col) => wide_col.col,
                None => continue,
            };

            let completion_item = CompletionItem {
                label,
                kind: Some(CompletionItemKind::TEXT),
                detail: Some(name.to_string()),
                insert_text: Some(emoji_char.to_string()),
                filter_text: Some(filter_text),
                sort_text: Some(format!("{:06}", completions.len())),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range: Range {
                        start: Position {
                            line: position.line,
                            character: colon_utf16,
                        },
                        end: position,
                    },
                    new_text: emoji_char.to_string(),
                })),
                ..Default::default()
            };

            completions.push(completion_item);

            if completions.len() >= 100 {
                break;
            }
        }
    }

    Some(CompletionResponse::Array(completions))
}
