mod matching;

use std::collections::HashMap;
use std::error::Error;

use line_index::{LineIndex, WideEncoding};
use lsp_server::{Connection, Message, Response};
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionList, CompletionOptions, CompletionParams,
    CompletionResponse, CompletionTextEdit, DidChangeTextDocumentParams, DidOpenTextDocumentParams,
    Position, Range, ServerCapabilities, TextDocumentSyncKind, TextEdit,
};

use matching::find_matching_emojis;

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
        return incomplete(vec![]);
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
        None => return incomplete(vec![]),
    };

    if byte_offset > line_text.len() {
        return incomplete(vec![]);
    }

    let (colon_pos, query) = match emoji_query(&line_text[..byte_offset]) {
        Some((pos, query)) => (pos, query.to_lowercase()),
        None => return incomplete(vec![]),
    };

    let scored_emojis = find_matching_emojis(&query);

    let completions: Vec<CompletionItem> = scored_emojis
        .iter()
        .filter_map(|scored| {
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

            // Convert UTF-8 byte offset back to UTF-16 for LSP
            let colon_utf16 = match line_index.to_wide(
                WideEncoding::Utf16,
                line_index::LineCol {
                    line: 0,
                    col: colon_pos as u32,
                },
            ) {
                Some(wide_col) => wide_col.col,
                None => return None,
            };

            Some(CompletionItem {
                label,
                kind: Some(CompletionItemKind::TEXT),
                detail: Some(scored.name.clone()),
                insert_text: Some(scored.emoji_char.clone()),
                filter_text: Some(filter_text),
                // Use negative score for sort_text (higher score = better match, lower sort value = appears first)
                sort_text: Some(format!("{:012}", u64::MAX - scored.score as u64)),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range: Range {
                        start: Position {
                            line: position.line,
                            character: colon_utf16,
                        },
                        end: position,
                    },
                    new_text: scored.emoji_char.clone(),
                })),
                ..Default::default()
            })
        })
        .collect();

    incomplete(completions)
}

/// Returns the byte position of the colon and the query after it, if the text before the
/// cursor ends in something like `:smi`.
fn emoji_query(before_cursor: &str) -> Option<(usize, &str)> {
    let query_start = before_cursor
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-')))
        .map_or(0, |pos| pos + 1);
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
    use lsp_types::{TextDocumentIdentifier, TextDocumentPositionParams, Url};

    const URI: &str = "file:///test.md";

    fn complete(line: &str) -> Option<CompletionResponse> {
        let documents = HashMap::from([(URI.to_string(), line.to_string())]);
        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse(URI).unwrap(),
                },
                position: Position {
                    line: 0,
                    character: line.encode_utf16().count() as u32,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        };
        handle_completion(&documents, params)
    }

    fn list(line: &str) -> lsp_types::CompletionList {
        match complete(line) {
            Some(CompletionResponse::List(list)) => list,
            other => panic!("expected a completion list, got {other:?}"),
        }
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
    fn ignores_query_with_space() {
        assert!(list(":sm ile").items.is_empty());
    }
}
