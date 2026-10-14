mod matching;
mod settings;
mod teams;

use std::collections::HashMap;
use std::error::Error;

use line_index::{LineIndex, WideEncoding};
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId, Response};
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionList, CompletionOptions, CompletionParams,
    CompletionRegistrationOptions, CompletionResponse, CompletionTextEdit,
    DidChangeConfigurationParams, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, Position, Range, Registration, RegistrationParams,
    ServerCapabilities, TextDocumentRegistrationOptions, TextDocumentSyncKind, TextEdit,
    Unregistration, UnregistrationParams,
};

use emojis::SkinTone;
use matching::{ScoredEmoji, find_matching_emojis, find_teams_emojis};
use settings::{Settings, Style};

fn main() -> Result<(), Box<dyn Error + Sync + Send>> {
    let (connection, io_threads) = Connection::stdio();

    let (id, initialize_params) = connection.initialize_start()?;
    let dynamic_registration = supports_dynamic_completion(&initialize_params);
    // Without dynamic registration `(` can only be declared now, before the settings are known.
    let capabilities = serde_json::to_value(server_capabilities(!dynamic_registration)).unwrap();
    connection.initialize_finish(id, serde_json::json!({ "capabilities": capabilities }))?;

    main_loop(connection, dynamic_registration)?;
    io_threads.join()?;
    Ok(())
}

fn server_capabilities(teams_trigger: bool) -> ServerCapabilities {
    // A space is a trigger so Zed asks again for the second word of a query like `:thumbs up`.
    let mut trigger_characters = vec![":".to_string(), " ".to_string()];
    if teams_trigger {
        trigger_characters.push("(".to_string());
    }
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncKind::FULL.into()),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(trigger_characters),
            resolve_provider: Some(false),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Returns whether the client accepts completion trigger characters registered after startup.
fn supports_dynamic_completion(initialize_params: &serde_json::Value) -> bool {
    initialize_params
        .pointer("/capabilities/textDocument/completion/dynamicRegistration")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

const TEAMS_REGISTRATION_ID: &str = "teams-trigger";

/// Builds the request that adds `(` as a completion trigger character, or removes it again.
fn teams_trigger_request(id: RequestId, register: bool) -> Request {
    let method = "textDocument/completion".to_string();
    if register {
        let options = CompletionRegistrationOptions {
            text_document_registration_options: TextDocumentRegistrationOptions {
                document_selector: None,
            },
            completion_options: CompletionOptions {
                trigger_characters: Some(vec!["(".to_string()]),
                resolve_provider: Some(false),
                ..Default::default()
            },
        };
        let params = RegistrationParams {
            registrations: vec![Registration {
                id: TEAMS_REGISTRATION_ID.to_string(),
                method,
                register_options: Some(serde_json::to_value(options).unwrap()),
            }],
        };
        Request::new(id, "client/registerCapability".to_string(), params)
    } else {
        let params = UnregistrationParams {
            unregisterations: vec![Unregistration {
                id: TEAMS_REGISTRATION_ID.to_string(),
                method,
            }],
        };
        Request::new(id, "client/unregisterCapability".to_string(), params)
    }
}

fn main_loop(
    connection: Connection,
    dynamic_registration: bool,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    let mut documents: HashMap<String, String> = HashMap::new();
    let mut settings = Settings::default();
    let mut teams_registered = false;
    let mut next_request_id = 0;

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
                            Ok((id, params)) => Response::new_ok(
                                id,
                                handle_completion(&documents, params, &settings),
                            ),
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
            // The server only sends trigger registrations, whose replies need no action.
            Message::Response(_) => {}
            Message::Notification(not) if not.method == "workspace/didChangeConfiguration" => {
                match not
                    .extract::<DidChangeConfigurationParams>("workspace/didChangeConfiguration")
                {
                    Ok(params) => {
                        settings = Settings::from_json(&params.settings);
                        let teams = settings.has(Style::Teams);
                        if dynamic_registration && teams != teams_registered {
                            next_request_id += 1;
                            let req =
                                teams_trigger_request(RequestId::from(next_request_id), teams);
                            connection.sender.send(Message::Request(req))?;
                            teams_registered = teams;
                        }
                    }
                    Err(e) => eprintln!("ignoring notification: {e}"),
                }
            }
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
    settings: &Settings,
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

    let Some((start, style, query)) = emoji_query(before_cursor) else {
        return incomplete(vec![]);
    };
    if !settings.has(style) {
        return incomplete(vec![]);
    }

    // Convert UTF-8 byte offset back to UTF-16 for LSP
    let Some(start_utf16) = line_index.to_wide(
        WideEncoding::Utf16,
        line_index::LineCol {
            line: 0,
            col: start as u32,
        },
    ) else {
        return incomplete(vec![]);
    };
    let mut end = position;
    // Zed closes `(` as it is typed, so a `)` right after the cursor would be left behind the emoji.
    if style == Style::Teams && line_text[byte_offset..].starts_with(')') {
        end.character += 1;
    }
    let range = Range {
        start: Position {
            line: position.line,
            character: start_utf16.col,
        },
        end,
    };

    let completions = match style {
        Style::Slack => slack_items(query, settings.skin_tone, range),
        Style::Emojisense => emojisense_items(query, range),
        Style::Teams => teams_items(query, settings.skin_tone, range),
    };
    incomplete(completions)
}

fn slack_items(query: &str, skin_tone: SkinTone, range: Range) -> Vec<CompletionItem> {
    find_matching_emojis(query, skin_tone)
        .iter()
        .enumerate()
        .map(|(rank, scored)| {
            let code = scored.shortcode.as_deref().unwrap_or(&scored.name);
            let label = format!(":{code} {}", scored.emoji_char);
            completion_item(rank, scored, label, scored.emoji_char.clone(), range)
        })
        .collect()
}

/// Shortcodes cannot carry a skin tone, so emojis are matched and shown without one.
fn emojisense_items(query: &str, range: Range) -> Vec<CompletionItem> {
    find_matching_emojis(query, SkinTone::Default)
        .iter()
        // Emojis without a shortcode have no markup form.
        .filter_map(|scored| Some((scored, scored.shortcode.as_deref()?)))
        .enumerate()
        .map(|(rank, (scored, code))| {
            let label = format!(":{code}: {}", scored.emoji_char);
            completion_item(rank, scored, label, format!(":{code}:"), range)
        })
        .collect()
}

fn teams_items(query: &str, skin_tone: SkinTone, range: Range) -> Vec<CompletionItem> {
    find_teams_emojis(query, skin_tone)
        .iter()
        .enumerate()
        .map(|(rank, scored)| {
            let label = match scored.teams_code {
                Some(code) => format!("({code}) {}", scored.emoji_char),
                None => format!(
                    "(:{} {}",
                    scored.shortcode.as_deref().unwrap_or(&scored.name),
                    scored.emoji_char
                ),
            };
            completion_item(rank, scored, label, scored.emoji_char.clone(), range)
        })
        .collect()
}

fn completion_item(
    rank: usize,
    scored: &ScoredEmoji,
    label: String,
    new_text: String,
    range: Range,
) -> CompletionItem {
    // Zed filters the items again as the user types, so a Teams code like `y` must be in the text it filters on.
    let code = scored.shortcode.as_deref().unwrap_or(&scored.name);
    let filter_text = match scored.teams_code {
        Some(teams_code) => format!("{teams_code} {code} {}", scored.name),
        None => format!("{code} {}", scored.name),
    };
    CompletionItem {
        label,
        kind: Some(CompletionItemKind::TEXT),
        detail: Some(scored.name.clone()),
        filter_text: Some(filter_text),
        sort_text: Some(sort_text(rank)),
        text_edit: Some(CompletionTextEdit::Edit(TextEdit { range, new_text })),
        ..Default::default()
    }
}

/// Zed sorts by comparing strings, so the rank is zero-padded to keep the order the results came in.
fn sort_text(rank: usize) -> String {
    format!("{rank:03}")
}

/// Returns where the replaced text starts, the style its prefix selects and the query, if the
/// text before the cursor ends in something like `:smi`, `::thumbs u` or `(lau`.
fn emoji_query(before_cursor: &str) -> Option<(usize, Style, &str)> {
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
    let prefix = &before_cursor[..query_start];
    let (start, style) = if let Some(rest) = prefix.strip_suffix("::") {
        (rest.len(), Style::Emojisense)
    } else if let Some(rest) = prefix.strip_suffix(':') {
        (rest.len(), Style::Slack)
    } else {
        (prefix.strip_suffix('(')?.len(), Style::Teams)
    };
    // Only a prefix that starts a word counts, so `std::fs`, `key:value` and `foo(bar` do not trigger.
    let previous = before_cursor[..start].chars().next_back();
    let starts_word = match style {
        Style::Teams => previous.is_none_or(char::is_whitespace),
        Style::Slack | Style::Emojisense => previous.is_none_or(|c| c.is_whitespace() || c == '('),
    };
    starts_word.then(|| (start, style, &before_cursor[query_start..]))
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

    type ServerThread = std::thread::JoinHandle<Result<(), Box<dyn Error + Sync + Send>>>;

    fn start_server(dynamic_registration: bool) -> (Connection, ServerThread) {
        let (server, client) = Connection::memory();
        (
            client,
            std::thread::spawn(move || main_loop(server, dynamic_registration)),
        )
    }

    fn notify(client: &Connection, method: &str, params: serde_json::Value) {
        let not = Notification::new(method.to_string(), params);
        client.sender.send(Message::Notification(not)).unwrap();
    }

    fn request(client: &Connection, id: i32, method: &str, params: serde_json::Value) -> Response {
        let req = lsp_server::Request::new(id.into(), method.to_string(), params);
        client.sender.send(Message::Request(req)).unwrap();
        match client.receiver.recv().unwrap() {
            Message::Response(resp) => resp,
            other => panic!("expected a response, got {other:?}"),
        }
    }

    fn did_open(client: &Connection, text: &str) {
        notify(
            client,
            "textDocument/didOpen",
            serde_json::json!({
                "textDocument": { "uri": URI, "languageId": "markdown", "version": 1, "text": text }
            }),
        );
    }

    fn inserted_text(resp: Response) -> String {
        let list: CompletionList = serde_json::from_value(resp.response_result.unwrap()).unwrap();
        match &list.items[0].text_edit {
            Some(CompletionTextEdit::Edit(edit)) => edit.new_text.clone(),
            other => panic!("expected a text edit, got {other:?}"),
        }
    }

    fn set_styles(client: &Connection, styles: &[&str]) {
        notify(
            client,
            "workspace/didChangeConfiguration",
            serde_json::json!({ "settings": { "styles": styles } }),
        );
    }

    fn next_request(client: &Connection) -> lsp_server::Request {
        match client.receiver.recv().unwrap() {
            Message::Request(req) => req,
            other => panic!("expected a request, got {other:?}"),
        }
    }

    #[test]
    fn registers_teams_trigger_while_teams_is_on() {
        let (client, server_thread) = start_server(true);

        set_styles(&client, &["teams"]);
        let req = next_request(&client);
        assert_eq!(req.method, "client/registerCapability");
        assert_eq!(
            req.params["registrations"][0]["registerOptions"]["triggerCharacters"],
            serde_json::json!(["("])
        );

        // The same setting again sends nothing, so the next request is the unregistration.
        set_styles(&client, &["slack", "teams"]);
        set_styles(&client, &["slack"]);
        let req = next_request(&client);
        assert_eq!(req.method, "client/unregisterCapability");
        assert_eq!(
            req.params["unregisterations"][0]["id"],
            TEAMS_REGISTRATION_ID
        );

        drop(client);
        server_thread.join().unwrap().unwrap();
    }

    #[test]
    fn does_not_register_without_client_support() {
        let (client, server_thread) = start_server(false);
        set_styles(&client, &["teams"]);
        did_open(&client, "(lau");
        let params = serde_json::to_value(completion_params(4)).unwrap();
        // A registration request would arrive before this response and fail the helper.
        let resp = request(&client, 1, "textDocument/completion", params);
        assert!(resp.response_result.is_ok());

        drop(client);
        server_thread.join().unwrap().unwrap();
    }

    #[test]
    fn declares_teams_trigger_at_startup_only_when_asked() {
        let triggers = |teams| {
            server_capabilities(teams)
                .completion_provider
                .unwrap()
                .trigger_characters
                .unwrap()
        };
        assert!(triggers(true).contains(&"(".to_string()));
        assert!(!triggers(false).contains(&"(".to_string()));
        assert!(triggers(false).contains(&":".to_string()));
    }

    #[test]
    fn reads_dynamic_registration_support() {
        let supported = serde_json::json!({
            "capabilities": { "textDocument": { "completion": { "dynamicRegistration": true } } }
        });
        assert!(supports_dynamic_completion(&supported));
        assert!(!supports_dynamic_completion(
            &serde_json::json!({ "capabilities": {} })
        ));
    }

    #[test]
    fn configuration_changes_apply_to_later_completions() {
        let (client, server_thread) = start_server(false);
        did_open(&client, ":thumbsup");
        let params = serde_json::to_value(completion_params(9)).unwrap();

        notify(
            &client,
            "workspace/didChangeConfiguration",
            serde_json::json!({ "settings": { "skin_tone": "dark" } }),
        );
        let resp = request(&client, 1, "textDocument/completion", params.clone());
        assert_eq!(inserted_text(resp), "👍🏿");

        notify(
            &client,
            "workspace/didChangeConfiguration",
            serde_json::json!({ "settings": null }),
        );
        let resp = request(&client, 2, "textDocument/completion", params);
        assert_eq!(inserted_text(resp), "👍");

        drop(client);
        server_thread.join().unwrap().unwrap();
    }

    fn complete_at(line: &str, character: u32) -> Option<CompletionResponse> {
        let documents = HashMap::from([(URI.to_string(), line.to_string())]);
        handle_completion(
            &documents,
            completion_params(character),
            &Settings::default(),
        )
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

    fn list_with(line: &str, settings: &Settings) -> lsp_types::CompletionList {
        let documents = HashMap::from([(URI.to_string(), line.to_string())]);
        let character = line.encode_utf16().count() as u32;
        match handle_completion(&documents, completion_params(character), settings) {
            Some(CompletionResponse::List(list)) => list,
            other => panic!("expected a completion list, got {other:?}"),
        }
    }

    fn all_styles() -> Settings {
        Settings {
            styles: vec![Style::Slack, Style::Emojisense, Style::Teams],
            ..Settings::default()
        }
    }

    fn edit_of(item: CompletionItem) -> TextEdit {
        match item.text_edit {
            Some(CompletionTextEdit::Edit(edit)) => edit,
            other => panic!("expected a text edit, got {other:?}"),
        }
    }

    fn first_edit(list: lsp_types::CompletionList) -> TextEdit {
        edit_of(list.items.into_iter().next().expect("expected an item"))
    }

    // Fuzzy scores can tie, so tests look items up by label instead of relying on their position.
    fn item_labeled(list: lsp_types::CompletionList, label: &str) -> CompletionItem {
        list.items
            .into_iter()
            .find(|item| item.label == label)
            .unwrap_or_else(|| panic!("expected an item labeled {label}"))
    }

    #[test]
    fn detects_each_style() {
        assert_eq!(emoji_query(":smi"), Some((0, Style::Slack, "smi")));
        assert_eq!(emoji_query("a ::smi"), Some((2, Style::Emojisense, "smi")));
        assert_eq!(emoji_query("(lau"), Some((0, Style::Teams, "lau")));
        assert_eq!(
            emoji_query("a (thumbs u"),
            Some((2, Style::Teams, "thumbs u"))
        );
        assert_eq!(emoji_query("(:smi"), Some((1, Style::Slack, "smi")));
        assert_eq!(emoji_query("(::smi"), Some((1, Style::Emojisense, "smi")));
    }

    #[test]
    fn ignores_prefixes_inside_words() {
        assert_eq!(emoji_query("foo(bar"), None);
        assert_eq!(emoji_query("((bar"), None);
        assert_eq!(emoji_query("std::fs"), None);
        assert_eq!(emoji_query(":::smi"), None);
    }

    #[test]
    fn style_that_is_off_gives_nothing() {
        assert!(list("::smi").items.is_empty());
        assert!(list("(lau").items.is_empty());
        let settings = Settings {
            styles: vec![],
            ..Settings::default()
        };
        assert!(list_with(":smi", &settings).items.is_empty());
    }

    #[test]
    fn emojisense_inserts_shortcode() {
        let edit = edit_of(item_labeled(
            list_with("foo ::smile", &all_styles()),
            ":smile: 😄",
        ));
        assert_eq!(edit.new_text, ":smile:");
        assert_eq!(edit.range.start.character, 4);
    }

    #[test]
    fn emojisense_ignores_skin_tone() {
        let settings = Settings {
            skin_tone: SkinTone::Dark,
            ..all_styles()
        };
        let edit = edit_of(item_labeled(
            list_with("::thumbsup", &settings),
            ":thumbsup: 👍",
        ));
        assert_eq!(edit.new_text, ":thumbsup:");
    }

    #[test]
    fn teams_inserts_emoji_for_code() {
        let item = item_labeled(list_with("(y", &all_styles()), "(y) 👍");
        assert!(item.filter_text.as_deref().unwrap().starts_with("y "));
        assert_eq!(edit_of(item).new_text, "👍");
    }

    #[test]
    fn teams_labels_name_matches() {
        item_labeled(list_with("(smile", &all_styles()), "(:smile 😄");
    }

    #[test]
    fn teams_replaces_closing_parenthesis() {
        let documents = HashMap::from([(URI.to_string(), "a (lau) b".to_string())]);
        let Some(CompletionResponse::List(list)) =
            handle_completion(&documents, completion_params(6), &all_styles())
        else {
            panic!("expected a completion list");
        };
        let edit = first_edit(list);
        assert_eq!(edit.range.start.character, 2);
        assert_eq!(edit.range.end.character, 7);
    }

    #[test]
    fn teams_range_ends_at_cursor_without_parenthesis() {
        let edit = first_edit(list_with("(lau", &all_styles()));
        assert_eq!(edit.range.end.character, 4);
    }

    #[test]
    fn teams_range_start_counts_utf16() {
        let edit = first_edit(list_with("😀 (lau", &all_styles()));
        assert_eq!(edit.range.start.character, 3);
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
        let server_thread = std::thread::spawn(move || main_loop(server, false));
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
    fn sort_text_keeps_result_order() {
        let ranks = [0, 1, 9, 10, 99];
        let mut by_text = ranks;
        by_text.sort_by_key(|&rank| sort_text(rank));
        assert_eq!(by_text, ranks);
    }

    #[test]
    fn inserts_emoji_with_skin_tone() {
        let documents = HashMap::from([(URI.to_string(), ":thumbsup".to_string())]);
        let response = handle_completion(
            &documents,
            completion_params(9),
            &Settings {
                skin_tone: SkinTone::Dark,
                ..Settings::default()
            },
        );
        let Some(CompletionResponse::List(list)) = response else {
            panic!("expected a completion list, got {response:?}");
        };
        match &list.items[0].text_edit {
            Some(CompletionTextEdit::Edit(edit)) => assert_eq!(edit.new_text, "👍🏿"),
            other => panic!("expected a text edit, got {other:?}"),
        }
    }
}
