use std::collections::HashMap;
use std::error::Error;

use line_index::{LineIndex, WideEncoding};
use lsp_server::{Connection, Message, Response};
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse,
    CompletionTextEdit, DidChangeTextDocumentParams, DidOpenTextDocumentParams, Position, Range,
    ServerCapabilities, TextDocumentSyncKind, TextEdit,
};
use nucleo_matcher::{Config, Matcher, Utf32Str};

#[derive(Debug, Clone)]
struct ScoredEmoji {
    emoji_char: String,
    name: String,
    shortcode: Option<String>,
    score: u32,
}

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

/// Finds and scores emojis matching the given query
fn find_matching_emojis(query: &str) -> Vec<ScoredEmoji> {
    if query.is_empty() {
        return vec![];
    }

    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut results = Vec::new();

    // Reusable buffers for UTF-32 conversion
    let mut haystack_buf = vec![];
    let mut needle_buf = vec![];

    for emoji in emojis::iter() {
        let emoji_char = emoji.as_str();
        let name = emoji.name();
        let shortcode = emoji.shortcode();

        // Try matching against shortcode first (higher priority), then name
        let (mut score, matched_field): (u32, &str) = if let Some(code) = shortcode {
            haystack_buf.clear();
            needle_buf.clear();
            let code_utf32 = Utf32Str::new(code, &mut haystack_buf);
            let query_utf32 = Utf32Str::new(query, &mut needle_buf);
            if let Some(s) = matcher.fuzzy_match(code_utf32, query_utf32) {
                (s as u32, code)
            } else {
                haystack_buf.clear();
                needle_buf.clear();
                let name_utf32 = Utf32Str::new(name, &mut haystack_buf);
                let query_utf32 = Utf32Str::new(query, &mut needle_buf);
                match matcher.fuzzy_match(name_utf32, query_utf32) {
                    Some(s) => (s as u32, name),
                    None => continue,
                }
            }
        } else {
            haystack_buf.clear();
            needle_buf.clear();
            let name_utf32 = Utf32Str::new(name, &mut haystack_buf);
            let query_utf32 = Utf32Str::new(query, &mut needle_buf);
            match matcher.fuzzy_match(name_utf32, query_utf32) {
                Some(s) => (s as u32, name),
                None => continue,
            }
        };

        // Boost score for exact matches and prefix matches
        let matched_field_lower = matched_field.to_lowercase();
        if matched_field_lower == query {
            // Exact match - huge boost
            score += 10000;
        } else if matched_field_lower.starts_with(query) {
            // Prefix match - significant boost
            score += 5000;
        }

        results.push(ScoredEmoji {
            emoji_char: emoji_char.to_string(),
            name: name.to_string(),
            shortcode: shortcode.map(|s| s.to_string()),
            score,
        });
    }

    // Sort by score (higher is better) and take top 100
    results.sort_by(|a, b| b.score.cmp(&a.score));
    results.truncate(100);

    results
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

    Some(CompletionResponse::Array(completions))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match_ranks_first() {
        let results = find_matching_emojis("smile");

        assert!(!results.is_empty(), "Should find emojis matching 'smile'");

        // The first result should be the exact match ":smile"
        let first = &results[0];
        assert_eq!(
            first.shortcode.as_deref(),
            Some("smile"),
            "First result should be exact match ':smile', but got {:?}",
            first.shortcode
        );
    }

    #[test]
    fn test_prefix_match_ranks_before_substring() {
        let results = find_matching_emojis("smile");

        // Find indices of different types of matches
        let exact_idx = results
            .iter()
            .position(|e| e.shortcode.as_deref() == Some("smile"));
        let prefix_idx = results.iter().position(|e| {
            e.shortcode
                .as_ref()
                .map_or(false, |s| s.starts_with("smile") && s != "smile")
        });
        let substring_idx = results.iter().position(|e| {
            e.shortcode
                .as_ref()
                .map_or(false, |s| s.contains("smile") && !s.starts_with("smile"))
        });

        // Exact match should come first
        assert!(exact_idx.is_some(), "Should have exact match");

        // If we have both prefix and substring matches, prefix should come first
        if let (Some(prefix), Some(substring)) = (prefix_idx, substring_idx) {
            assert!(
                prefix < substring,
                "Prefix matches should rank before substring matches"
            );
        }
    }

    #[test]
    fn test_heart_exact_match() {
        let results = find_matching_emojis("heart");

        assert!(!results.is_empty(), "Should find emojis matching 'heart'");

        // The first result should be the exact match ":heart"
        let first = &results[0];
        assert_eq!(
            first.shortcode.as_deref(),
            Some("heart"),
            "First result should be exact match ':heart', but got {:?}",
            first.shortcode
        );
    }

    #[test]
    fn test_cat_exact_match() {
        let results = find_matching_emojis("cat");

        assert!(!results.is_empty(), "Should find emojis matching 'cat'");

        // The first result should be the exact match ":cat"
        let first = &results[0];
        assert_eq!(
            first.shortcode.as_deref(),
            Some("cat"),
            "First result should be exact match ':cat', but got {:?}",
            first.shortcode
        );
    }

    #[test]
    fn test_empty_query_returns_nothing() {
        let results = find_matching_emojis("");
        assert!(results.is_empty(), "Empty query should return no results");
    }

    #[test]
    fn test_results_limited_to_100() {
        let results = find_matching_emojis("e");
        assert!(
            results.len() <= 100,
            "Results should be limited to 100, got {}",
            results.len()
        );
    }

    #[test]
    fn test_scores_are_ordered() {
        let results = find_matching_emojis("smile");

        // Verify that results are sorted by score (descending)
        for i in 1..results.len() {
            assert!(
                results[i - 1].score >= results[i].score,
                "Results should be sorted by score (descending), but item {} has score {} and item {} has score {}",
                i - 1,
                results[i - 1].score,
                i,
                results[i].score
            );
        }
    }

    #[test]
    fn test_smile_ranks_before_sweat_smile() {
        let results = find_matching_emojis("smile");

        let smile_idx = results
            .iter()
            .position(|e| e.shortcode.as_deref() == Some("smile"));
        let sweat_smile_idx = results
            .iter()
            .position(|e| e.shortcode.as_deref() == Some("sweat_smile"));
        let kissing_smile_eyes_idx = results
            .iter()
            .position(|e| e.shortcode.as_deref() == Some("kissing_smiling_eyes"));

        assert!(smile_idx.is_some(), "Should find ':smile' emoji");

        if let Some(smile_pos) = smile_idx {
            if let Some(sweat_pos) = sweat_smile_idx {
                assert!(
                    smile_pos < sweat_pos,
                    "':smile' (pos {}) should rank before ':sweat_smile' (pos {}), scores: {} vs {}",
                    smile_pos,
                    sweat_pos,
                    results[smile_pos].score,
                    results[sweat_pos].score
                );
            }

            if let Some(kissing_pos) = kissing_smile_eyes_idx {
                assert!(
                    smile_pos < kissing_pos,
                    "':smile' (pos {}) should rank before ':kissing_smiling_eyes' (pos {}), scores: {} vs {}",
                    smile_pos,
                    kissing_pos,
                    results[smile_pos].score,
                    results[kissing_pos].score
                );
            }
        }
    }
}
