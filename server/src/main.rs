mod matching;

use std::collections::HashMap;
use std::error::Error;

use line_index::{LineIndex, WideEncoding};
use lsp_server::{Connection, ErrorCode, Message, Notification, Response};
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionList, CompletionOptions, CompletionParams,
    CompletionResponse, CompletionTextEdit, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, Position, Range, ServerCapabilities,
    TextDocumentSyncKind, TextEdit,
};

use matching::find_matching_emojis;

fn main() -> Result<(), Box<dyn Error + Sync + Send>> {
    let (connection, io_threads) = Connection::stdio();

    let server_capabilities = serde_json::to_value(&ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncKind::FULL.into()),
        completion_provider: Some(CompletionOptions {
            // A space is a trigger so Zed asks again for the second word of a query like `:thumbs up`.
            trigger_characters: Some(vec![":".to_string(), " ".to_string()]),
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
                let id = req.id.clone();
                let resp = match req.method.as_str() {
                    "textDocument/completion" => {
                        match req.extract::<CompletionParams>("textDocument/completion") {
                            Ok((id, params)) => {
                                Response::new_ok(id, handle_completion(&documents, params))
                            }
                            Err(e) => Response::new_err(
                                id,
                                ErrorCode::InvalidParams as i32,
                                e.to_string(),
                            ),
                        }
                    }
                    method => Response::new_err(
                        id,
                        ErrorCode::MethodNotFound as i32,
                        format!("unhandled method {method}"),
                    ),
                };
                connection.sender.send(Message::Response(resp))?;
            }
            Message::Response(_) => {}
            Message::Notification(not) => {
                // A malformed notification should not take down completions for every open file.
                if let Err(e) = handle_notification(&mut documents, not) {
                    eprintln!("ignoring notification: {e}");
                }
            }
        }
    }
    Ok(())
}

fn handle_notification(
    documents: &mut HashMap<String, String>,
    not: Notification,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    match not.method.as_str() {
        "textDocument/didOpen" => {
            let params = not.extract::<DidOpenTextDocumentParams>("textDocument/didOpen")?;
            documents.insert(
                params.text_document.uri.to_string(),
                params.text_document.text,
            );
        }
        "textDocument/didChange" => {
            let params = not.extract::<DidChangeTextDocumentParams>("textDocument/didChange")?;
            if let Some(change) = params.content_changes.into_iter().next() {
                documents.insert(params.text_document.uri.to_string(), change.text);
            }
        }
        "textDocument/didClose" => {
            let params = not.extract::<DidCloseTextDocumentParams>("textDocument/didClose")?;
            documents.remove(params.text_document.uri.as_str());
        }
        _ => {}
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

    let Some(line_text) = text.lines().nth(line_idx) else {
        return incomplete(vec![]);
    };

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
        None => return incomplete(vec![]),
    };

    // A position past the end of the line or inside a surrogate pair gives no valid byte offset.
    let Some(before_cursor) = line_text.get(..byte_offset) else {
        return incomplete(vec![]);
    };

    let Some((colon_pos, query)) = emoji_query(before_cursor) else {
        return incomplete(vec![]);
    };

    // Convert UTF-8 byte offset back to UTF-16 for LSP
    let Some(colon_utf16) = line_index.to_wide(
        WideEncoding::Utf16,
        line_index::LineCol {
            line: 0,
            col: colon_pos as u32,
        },
    ) else {
        return incomplete(vec![]);
    };

    let scored_emojis = find_matching_emojis(query);

    let completions: Vec<CompletionItem> = scored_emojis
        .iter()
        .map(|scored| {
            let label = if let Some(code) = &scored.shortcode {
                format!(":{} {}", code, scored.emoji_char)
            } else {
                format!(":{} {}", scored.name, scored.emoji_char)
            };

            let filter_text = format!(
                "{} {}",
                scored.shortcode.as_deref().unwrap_or(&scored.name),
                scored.name
            );

            CompletionItem {
                label,
                kind: Some(CompletionItemKind::TEXT),
                detail: Some(scored.name.clone()),
                filter_text: Some(filter_text),
                sort_text: Some(sort_text(scored.score)),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range: Range {
                        start: Position {
                            line: position.line,
                            character: colon_utf16.col,
                        },
                        end: position,
                    },
                    new_text: scored.emoji_char.clone(),
                })),
                ..Default::default()
            }
        })
        .collect();

    incomplete(completions)
}

/// Zed sorts by comparing strings, so higher scores must give strings that sort earlier.
fn sort_text(score: u32) -> String {
    format!("{:010}", u32::MAX - score)
}

/// Returns the byte position of the colon and the query after it, if the text before the
/// cursor ends in something like `:smi` or `:thumbs u`.
fn emoji_query(before_cursor: &str) -> Option<(usize, &str)> {
    let last_word_start = word_start(before_cursor);
    let query_start = match before_cursor[..last_word_start].strip_suffix(' ') {
        // The query is limited to two words, so text typed after an emoji shortcode stops
        // triggering completions once the next word ends.
        Some(before_space) => {
            let first_word_start = word_start(before_space);
            if first_word_start == before_space.len() {
                return None;
            }
            first_word_start
        }
        None => last_word_start,
    };
    let colon_pos = query_start.checked_sub(1)?;
    if !before_cursor[colon_pos..].starts_with(':') {
        return None;
    }
    // Only a colon that starts a word counts, so `std::fs` and `key:value` do not trigger.
    let starts_word = before_cursor[..colon_pos]
        .chars()
        .next_back()
        .is_none_or(|c| c.is_whitespace() || c == '(');
    starts_word.then(|| (colon_pos, &before_cursor[query_start..]))
}

/// Returns the byte position where the shortcode-like word at the end of `text` starts.
fn word_start(text: &str) -> usize {
    text.rfind(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-')))
        .map_or(0, |pos| pos + 1)
}

// Results are capped, so Zed must ask again as the query grows instead of filtering the first answer.
fn incomplete(items: Vec<CompletionItem>) -> Option<CompletionResponse> {
    Some(CompletionResponse::List(CompletionList {
        is_incomplete: true,
        items,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{TextDocumentIdentifier, TextDocumentPositionParams, Uri};

    const URI: &str = "file:///test.md";

    fn completion_params(character: u32) -> CompletionParams {
        CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: URI.parse::<Uri>().unwrap(),
                },
                position: Position { line: 0, character },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        }
    }

    fn complete_at(line: &str, character: u32) -> Option<CompletionResponse> {
        let documents = HashMap::from([(URI.to_string(), line.to_string())]);
        handle_completion(&documents, completion_params(character))
    }

    fn list_at(line: &str, character: u32) -> lsp_types::CompletionList {
        match complete_at(line, character) {
            Some(CompletionResponse::List(list)) => list,
            other => panic!("expected a completion list, got {other:?}"),
        }
    }

    fn list(line: &str) -> lsp_types::CompletionList {
        list_at(line, line.encode_utf16().count() as u32)
    }

    #[test]
    fn response_is_marked_incomplete() {
        assert!(list(":smi").is_incomplete);
    }

    #[test]
    fn empty_query_is_marked_incomplete() {
        let list = list(":");
        assert!(list.items.is_empty());
        assert!(list.is_incomplete);
    }

    fn has_shortcode(line: &str, shortcode: &str) -> bool {
        let label = format!(":{shortcode} ");
        list(line)
            .items
            .iter()
            .any(|item| item.label.starts_with(&label))
    }

    #[test]
    fn triggers_at_line_start() {
        assert!(has_shortcode(":smi", "smile"));
    }

    #[test]
    fn triggers_after_whitespace() {
        assert!(has_shortcode("foo :smi", "smile"));
    }

    #[test]
    fn triggers_after_parenthesis() {
        assert!(has_shortcode("(:smi", "smile"));
    }

    #[test]
    fn ignores_path_separator() {
        assert!(list("use std::fs").items.is_empty());
    }

    #[test]
    fn ignores_url() {
        assert!(list("http://x").items.is_empty());
    }

    #[test]
    fn ignores_colon_after_word() {
        assert!(list("key:value").items.is_empty());
    }

    #[test]
    fn matches_two_words() {
        assert!(has_shortcode(":thumbs u", "thumbsup"));
    }

    #[test]
    fn matches_after_trailing_space() {
        assert!(has_shortcode(":smile ", "smile"));
    }

    #[test]
    fn replaces_both_words() {
        let item = list("foo :thumbs u").items.into_iter().next().unwrap();
        match item.text_edit {
            Some(CompletionTextEdit::Edit(edit)) => assert_eq!(edit.range.start.character, 4),
            other => panic!("expected a text edit, got {other:?}"),
        }
    }

    #[test]
    fn ignores_third_word() {
        assert!(list(":smile and t").items.is_empty());
    }

    #[test]
    fn ignores_space_after_colon() {
        assert!(list(": smi").items.is_empty());
    }

    #[test]
    fn ignores_double_space() {
        assert!(list(":smile  t").items.is_empty());
    }

    #[test]
    fn did_close_forgets_document() {
        let mut documents = HashMap::new();
        let open = Notification::new(
            "textDocument/didOpen".to_string(),
            serde_json::json!({
                "textDocument": { "uri": URI, "languageId": "markdown", "version": 1, "text": ":smi" }
            }),
        );
        let close = Notification::new(
            "textDocument/didClose".to_string(),
            serde_json::json!({ "textDocument": { "uri": URI } }),
        );

        handle_notification(&mut documents, open).unwrap();
        assert!(documents.contains_key(URI));
        handle_notification(&mut documents, close).unwrap();
        assert!(!documents.contains_key(URI));
    }

    #[test]
    fn ignores_position_inside_surrogate_pair() {
        assert!(list_at("😀:smi", 1).items.is_empty());
    }

    #[test]
    fn ignores_position_past_line_end() {
        assert!(list_at(":smi", 10).items.is_empty());
    }

    #[test]
    fn answers_bad_requests_and_keeps_running() {
        use lsp_server::{Request, RequestId};

        let (server, client) = Connection::memory();
        let server_thread = std::thread::spawn(move || main_loop(server));
        let send_request = |id: i32, method: &str, params: serde_json::Value| {
            let req = Request::new(RequestId::from(id), method.to_string(), params);
            client.sender.send(Message::Request(req)).unwrap();
            match client.receiver.recv().unwrap() {
                Message::Response(resp) => resp,
                other => panic!("expected a response, got {other:?}"),
            }
        };

        let resp = send_request(1, "textDocument/hover", serde_json::json!({}));
        assert_eq!(
            resp.response_result.unwrap_err().code,
            ErrorCode::MethodNotFound as i32
        );

        let resp = send_request(2, "textDocument/completion", serde_json::json!({}));
        assert_eq!(
            resp.response_result.unwrap_err().code,
            ErrorCode::InvalidParams as i32
        );

        let bad_notification =
            Notification::new("textDocument/didOpen".to_string(), serde_json::json!({}));
        client
            .sender
            .send(Message::Notification(bad_notification))
            .unwrap();

        let params = serde_json::to_value(completion_params(0)).unwrap();
        let resp = send_request(3, "textDocument/completion", params);
        assert!(resp.response_result.is_ok());

        drop(client);
        server_thread.join().unwrap().unwrap();
    }

    #[test]
    fn sort_text_orders_higher_scores_first() {
        let scores = [0, 1, 9, 10, 99, 100, 1000, u16::MAX as u32, u32::MAX];
        let mut by_text = scores;
        by_text.sort_by_key(|&score| sort_text(score));
        let mut by_score = scores;
        by_score.sort_by(|a, b| b.cmp(a));
        assert_eq!(by_text, by_score);
    }
}
