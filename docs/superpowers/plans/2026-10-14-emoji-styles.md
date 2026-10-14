# Emoji Styles Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add emojisense (`::smi` → `:smile:`) and Teams (`(lau` → 😆) completion styles next to the existing Slack style, each turned on or off through `lsp.emoji-language-server.settings`, applied without a server restart.

**Architecture:** The extension passes the user's `settings` object to Zed as workspace configuration. Zed sends it to the server in `workspace/didChangeConfiguration` at startup and on every change. The server keeps a `Settings` value, `emoji_query` decides the style from the characters before the query, and `handle_completion` builds the items for that style. `(` is added as a trigger character through `client/registerCapability` while Teams is on.

**Tech Stack:** Rust 2024, `lsp-server` 0.10, `lsp-types` 0.97, `emojis` 0.9, `nucleo-matcher` 0.3, `zed_extension_api` 0.7.

**Spec:** `docs/superpowers/specs/2026-10-14-emoji-styles-design.md`

## Global Constraints

- Style names in settings are exactly `"slack"`, `"emojisense"` and `"teams"`. A missing `styles` means `["slack"]`.
- `skin_tone` values stay `default`, `light`, `medium-light`, `medium`, `medium-dark` and `dark`, and are read from `settings`, not from `initialization_options`.
- Unknown setting values are logged to stderr with `eprintln!` and ignored.
- Results stay capped at 100 items, and responses stay `is_incomplete: true`.
- CI runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. All three must pass after every task.
- Comments follow the repository style: one plain sentence that explains why, and never describes the change.

## Review Focus

- `didChangeConfiguration` with `settings: null`, or `styles` given as a string instead of a list, gives the default settings. Pinned in Task 1.
- Sending the same Teams setting twice sends no second `client/registerCapability`. Pinned in Task 4.
- A multi-byte character before `(` (`😀 (lau`) still gives a correct UTF-16 range start. Pinned in Task 3.
- With a dark skin tone, emojisense still inserts `:thumbsup:` with no tone. Pinned in Task 2.
- A Teams code that is not a word in the emoji's name (`(y` → 👍) survives Zed's own filtering, because the code is in `filter_text`. Pinned in Task 3.

---

### Task 1: Read settings from workspace configuration

**Files:**
- Create: `server/src/settings.rs`
- Modify: `server/src/main.rs` (imports, `main`, `skin_tone_setting` removed, `main_loop`, `handle_completion`, tests)
- Modify: `src/lib.rs` (new `language_server_workspace_configuration` in the `zed::Extension` impl)
- Modify: `README.md` (the "Skin tone" section)

**Interfaces:**
- Produces, in `server/src/settings.rs`:
  - `pub enum Style { Slack, Emojisense, Teams }` deriving `Debug, Clone, Copy, PartialEq, Eq`
  - `pub struct Settings { pub styles: Vec<Style>, pub skin_tone: SkinTone }` deriving `Debug, Clone, PartialEq`, with `Default` giving `styles: vec![Style::Slack], skin_tone: SkinTone::Default`
  - `impl Settings { pub fn from_json(value: &serde_json::Value) -> Settings; pub fn has(&self, style: Style) -> bool }`
- Produces, in `server/src/main.rs`:
  - `fn main_loop(connection: Connection) -> Result<(), Box<dyn Error + Sync + Send>>`
  - `fn handle_completion(documents: &HashMap<String, String>, params: CompletionParams, settings: &Settings) -> Option<CompletionResponse>`
  - Test helpers `start_server`, `notify`, `request` and `did_open` (below), used by Task 4.

- [ ] **Step 1: Write `server/src/settings.rs` with failing tests only**

```rust
use emojis::SkinTone;
use serde_json::Value;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_settings_are_default() {
        assert_eq!(Settings::from_json(&Value::Null), Settings::default());
        assert_eq!(
            Settings::from_json(&serde_json::json!({})),
            Settings::default()
        );
        assert_eq!(Settings::default().styles, vec![Style::Slack]);
        assert_eq!(Settings::default().skin_tone, SkinTone::Default);
    }

    #[test]
    fn reads_styles() {
        let settings =
            Settings::from_json(&serde_json::json!({ "styles": ["emojisense", "teams"] }));
        assert_eq!(settings.styles, vec![Style::Emojisense, Style::Teams]);
        assert!(!settings.has(Style::Slack));
        assert!(settings.has(Style::Teams));
    }

    #[test]
    fn empty_styles_turn_completions_off() {
        let settings = Settings::from_json(&serde_json::json!({ "styles": [] }));
        assert!(settings.styles.is_empty());
    }

    #[test]
    fn unknown_styles_are_ignored() {
        let settings = Settings::from_json(&serde_json::json!({ "styles": ["teams", "irc", 3] }));
        assert_eq!(settings.styles, vec![Style::Teams]);
    }

    #[test]
    fn styles_that_are_not_a_list_are_default() {
        let settings = Settings::from_json(&serde_json::json!({ "styles": "teams" }));
        assert_eq!(settings.styles, vec![Style::Slack]);
    }

    #[test]
    fn reads_each_skin_tone() {
        let cases = [
            ("default", SkinTone::Default),
            ("light", SkinTone::Light),
            ("medium-light", SkinTone::MediumLight),
            ("medium", SkinTone::Medium),
            ("medium-dark", SkinTone::MediumDark),
            ("dark", SkinTone::Dark),
        ];
        for (name, tone) in cases {
            let settings = Settings::from_json(&serde_json::json!({ "skin_tone": name }));
            assert_eq!(settings.skin_tone, tone, "{name}");
        }
    }

    #[test]
    fn unknown_skin_tone_is_default() {
        let settings = Settings::from_json(&serde_json::json!({ "skin_tone": "purple" }));
        assert_eq!(settings.skin_tone, SkinTone::Default);
        let settings = Settings::from_json(&serde_json::json!({ "skin_tone": 3 }));
        assert_eq!(settings.skin_tone, SkinTone::Default);
    }
}
```

Add `mod settings;` below `mod matching;` at the top of `server/src/main.rs`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p emoji-language-server settings::`
Expected: compile errors, `cannot find type Settings` and `cannot find type Style`.

- [ ] **Step 3: Implement `Settings` above the tests in `server/src/settings.rs`**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Slack,
    Emojisense,
    Teams,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub styles: Vec<Style>,
    pub skin_tone: SkinTone,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            styles: vec![Style::Slack],
            skin_tone: SkinTone::Default,
        }
    }
}

impl Settings {
    /// Reads the settings Zed sends, using the default for any value that is missing or unknown.
    pub fn from_json(value: &Value) -> Settings {
        Settings {
            styles: styles_setting(value.get("styles")),
            skin_tone: skin_tone_setting(value.get("skin_tone")),
        }
    }

    pub fn has(&self, style: Style) -> bool {
        self.styles.contains(&style)
    }
}

fn styles_setting(value: Option<&Value>) -> Vec<Style> {
    let Some(value) = value else {
        return Settings::default().styles;
    };
    let Some(entries) = value.as_array() else {
        eprintln!("ignoring styles {value}, which is not a list");
        return Settings::default().styles;
    };
    entries
        .iter()
        .filter_map(|entry| match entry.as_str() {
            Some("slack") => Some(Style::Slack),
            Some("emojisense") => Some(Style::Emojisense),
            Some("teams") => Some(Style::Teams),
            _ => {
                eprintln!("ignoring unknown style {entry}");
                None
            }
        })
        .collect()
}

fn skin_tone_setting(value: Option<&Value>) -> SkinTone {
    let Some(value) = value else {
        return SkinTone::Default;
    };
    match value.as_str() {
        Some("default") => SkinTone::Default,
        Some("light") => SkinTone::Light,
        Some("medium-light") => SkinTone::MediumLight,
        Some("medium") => SkinTone::Medium,
        Some("medium-dark") => SkinTone::MediumDark,
        Some("dark") => SkinTone::Dark,
        _ => {
            eprintln!("ignoring unknown skin_tone {value}");
            SkinTone::Default
        }
    }
}
```

- [ ] **Step 4: Run the settings tests**

Run: `cargo test -p emoji-language-server settings::`
Expected: all 7 pass. Clippy will warn that `Settings` is unused until Step 7, which is fine at this point.

- [ ] **Step 5: Write the failing protocol test in `server/src/main.rs`**

Add these helpers inside `mod tests`, after `completion_params`:

```rust
    type ServerThread = std::thread::JoinHandle<Result<(), Box<dyn Error + Sync + Send>>>;

    fn start_server() -> (Connection, ServerThread) {
        let (server, client) = Connection::memory();
        (client, std::thread::spawn(move || main_loop(server)))
    }

    fn notify(client: &Connection, method: &str, params: serde_json::Value) {
        let not = Notification::new(method.to_string(), params);
        client.sender.send(Message::Notification(not)).unwrap();
    }

    fn request(
        client: &Connection,
        id: i32,
        method: &str,
        params: serde_json::Value,
    ) -> Response {
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
```

Add the test:

```rust
    #[test]
    fn configuration_changes_apply_to_later_completions() {
        let (client, server_thread) = start_server();
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
```

- [ ] **Step 6: Run the test to verify it fails**

Run: `cargo test -p emoji-language-server configuration_changes_apply_to_later_completions`
Expected: compile error, `main_loop` takes 2 arguments but 1 was supplied.

- [ ] **Step 7: Use `Settings` in `server/src/main.rs`**

Replace `use emojis::SkinTone;` with:

```rust
use settings::Settings;
```

Add `DidChangeConfigurationParams` to the `lsp_types` import list.

Replace the two lines starting at `let initialization_params = connection.initialize(server_capabilities)?;` in `main` with:

```rust
    connection.initialize(server_capabilities)?;
    main_loop(connection)?;
```

Delete `skin_tone_setting` and its doc comment.

Change `main_loop` to:

```rust
fn main_loop(connection: Connection) -> Result<(), Box<dyn Error + Sync + Send>> {
    let mut documents: HashMap<String, String> = HashMap::new();
    let mut settings = Settings::default();

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
            Message::Response(_) => {}
            Message::Notification(not) if not.method == "workspace/didChangeConfiguration" => {
                match not.extract::<DidChangeConfigurationParams>("workspace/didChangeConfiguration")
                {
                    Ok(params) => settings = Settings::from_json(&params.settings),
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
```

In `handle_completion`, change the parameter `skin_tone: SkinTone` to `settings: &Settings`, and the call to `find_matching_emojis(query, settings.skin_tone)`.

In the tests:
- add `use emojis::SkinTone;` below `use super::*;`;
- change `complete_at` to call `handle_completion(&documents, completion_params(character), &Settings::default())`;
- in `inserts_emoji_with_skin_tone`, replace `SkinTone::Dark` with `&Settings { skin_tone: SkinTone::Dark, ..Settings::default() }`;
- in `answers_bad_requests_and_keeps_running`, change `main_loop(server, SkinTone::Default)` to `main_loop(server)`;
- delete `reads_each_skin_tone`, `missing_skin_tone_is_default` and `unknown_skin_tone_is_default`, which `settings.rs` now covers.

- [ ] **Step 8: Pass the settings from the extension in `src/lib.rs`**

Add to `impl zed::Extension for EmojiCompletionsExtension`, after `language_server_command`:

```rust
    fn language_server_workspace_configuration(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Option<zed::serde_json::Value>> {
        // Zed sends this to the server on start and on every settings change, so it needs no restart.
        Ok(LspSettings::for_worktree("emoji-language-server", worktree)
            .ok()
            .and_then(|settings| settings.settings))
    }
```

- [ ] **Step 9: Update the README "Skin tone" section**

Replace the JSON block and the paragraph after it with:

````markdown
```json
"lsp": {
  "emoji-language-server": {
    "settings": { "skin_tone": "medium-dark" }
  }
}
```

The values are `default` (yellow, used when the setting is missing), `light`, `medium-light`, `medium`, `medium-dark` and `dark`.
````

- [ ] **Step 10: Run all checks**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: no warnings, all tests pass.

- [ ] **Step 11: Commit**

```bash
git add server/src/settings.rs server/src/main.rs src/lib.rs README.md
git commit -m "Read settings from workspace configuration"
```

---

### Task 2: Detect the style from the prefix, and add emojisense

**Files:**
- Modify: `server/src/main.rs` (`emoji_query`, `handle_completion`, `sort_text`, new `slack_items`, `emojisense_items`, `completion_item`, tests)

**Interfaces:**
- Consumes: `Settings`, `Settings::has`, `Style` from Task 1.
- Produces:
  - `fn emoji_query(before_cursor: &str) -> Option<(usize, Style, &str)>`, which returns the byte position where the replaced text starts, the style and the query.
  - Test helpers `edit_of(item: CompletionItem) -> TextEdit`, `first_edit(list: CompletionList) -> TextEdit` and `item_labeled(list: CompletionList, label: &str) -> CompletionItem`.
  - `fn completion_item(rank: usize, scored: &ScoredEmoji, label: String, new_text: String, range: Range) -> CompletionItem`
  - `fn sort_text(rank: usize) -> String`
  - Test helper `fn list_with(line: &str, settings: &Settings) -> CompletionList`, and `fn all_styles() -> Settings`, used by Task 3.

- [ ] **Step 1: Write the failing tests**

Add helpers inside `mod tests`, after `list`:

```rust
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
```

Add tests:

```rust
    #[test]
    fn detects_each_style() {
        assert_eq!(emoji_query(":smi"), Some((0, Style::Slack, "smi")));
        assert_eq!(emoji_query("a ::smi"), Some((2, Style::Emojisense, "smi")));
        assert_eq!(emoji_query("(lau"), Some((0, Style::Teams, "lau")));
        assert_eq!(emoji_query("a (thumbs u"), Some((2, Style::Teams, "thumbs u")));
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
        let edit = edit_of(item_labeled(list_with("foo ::smile", &all_styles()), ":smile: 😄"));
        assert_eq!(edit.new_text, ":smile:");
        assert_eq!(edit.range.start.character, 4);
    }

    #[test]
    fn emojisense_ignores_skin_tone() {
        let settings = Settings {
            skin_tone: SkinTone::Dark,
            ..all_styles()
        };
        let edit = edit_of(item_labeled(list_with("::thumbsup", &settings), ":thumbsup: 👍"));
        assert_eq!(edit.new_text, ":thumbsup:");
    }

```

Replace `sort_text_orders_higher_scores_first` with:

```rust
    #[test]
    fn sort_text_keeps_result_order() {
        let ranks = [0, 1, 9, 10, 99];
        let mut by_text = ranks;
        by_text.sort_by_key(|&rank| sort_text(rank));
        assert_eq!(by_text, ranks);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p emoji-language-server`
Expected: compile errors, because `emoji_query` returns a 2-tuple and `sort_text` takes a `u32`.

- [ ] **Step 3: Rewrite `emoji_query`**

Replace `emoji_query` and its doc comment with:

```rust
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
    } else if let Some(rest) = prefix.strip_suffix('(') {
        (rest.len(), Style::Teams)
    } else {
        return None;
    };
    // Only a prefix that starts a word counts, so `std::fs`, `key:value` and `foo(bar` do not trigger.
    let previous = before_cursor[..start].chars().next_back();
    let starts_word = match style {
        Style::Teams => previous.is_none_or(char::is_whitespace),
        Style::Slack | Style::Emojisense => {
            previous.is_none_or(|c| c.is_whitespace() || c == '(')
        }
    };
    starts_word.then(|| (start, style, &before_cursor[query_start..]))
}
```

Add `use settings::{Settings, Style};` in place of `use settings::Settings;`, and add `use matching::ScoredEmoji;` and `use emojis::SkinTone;` to the imports at the top. Remove the `use emojis::SkinTone;` line from the test module, since `use super::*;` now brings it in.

- [ ] **Step 4: Build items per style in `handle_completion`**

Replace everything in `handle_completion` from `let Some((colon_pos, query)) = emoji_query(before_cursor) else {` to the end of the function with:

```rust
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
    let range = Range {
        start: Position {
            line: position.line,
            character: start_utf16.col,
        },
        end: position,
    };

    let completions = match style {
        Style::Slack => slack_items(query, settings.skin_tone, range),
        Style::Emojisense => emojisense_items(query, range),
        Style::Teams => Vec::new(),
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

fn completion_item(
    rank: usize,
    scored: &ScoredEmoji,
    label: String,
    new_text: String,
    range: Range,
) -> CompletionItem {
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
        sort_text: Some(sort_text(rank)),
        text_edit: Some(CompletionTextEdit::Edit(TextEdit { range, new_text })),
        ..Default::default()
    }
}
```

Replace `sort_text` and its doc comment with:

```rust
/// Zed sorts by comparing strings, so the rank is zero-padded to keep the order the results came in.
fn sort_text(rank: usize) -> String {
    format!("{rank:03}")
}
```

- [ ] **Step 5: Run all checks**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all pass, including the existing Slack tests (`replaces_both_words`, `ignores_path_separator` and the others).

- [ ] **Step 6: Commit**

```bash
git add server/src/main.rs
git commit -m "Detect the style from the prefix and add emojisense markup"
```

---

### Task 3: Teams codes

**Files:**
- Create: `server/src/teams.rs`
- Modify: `server/src/matching.rs` (`ScoredEmoji.teams_code`, `query_pattern`, `find_teams_emojis`, tests)
- Modify: `server/src/main.rs` (`mod teams;`, `teams_items`, the `)` range, `completion_item` filter text, `(` trigger character, tests)

**Interfaces:**
- Consumes: `completion_item`, `list_with`, `all_styles` and `first_edit` from Task 2.
- Produces:
  - `pub const TEAMS_CODES: &[(&str, &str)]` in `server/src/teams.rs`
  - `pub teams_code: Option<&'static str>` on `ScoredEmoji`
  - `pub fn find_teams_emojis(query: &str, skin_tone: SkinTone) -> Vec<ScoredEmoji>` in `server/src/matching.rs`

- [ ] **Step 1: Create `server/src/teams.rs` with its tests**

```rust
/// Classic Skype and Teams codes, typed as `(code)`, with the Unicode emoji each one stands for.
/// Codes for Teams' own animated emojis are left out because they have no Unicode equivalent.
pub const TEAMS_CODES: &[(&str, &str)] = &[
    ("smile", "🙂"),
    ("happy", "😃"),
    ("laugh", "😄"),
    ("lol", "😂"),
    ("cwl", "😂"),
    ("rofl", "🤣"),
    ("giggle", "🤭"),
    ("wink", "😉"),
    ("blush", "😊"),
    ("cool", "😎"),
    ("inlove", "😍"),
    ("kiss", "😘"),
    ("tongueout", "😛"),
    ("mmm", "😋"),
    ("nerd", "🤓"),
    ("smirk", "😏"),
    ("angel", "😇"),
    ("party", "🥳"),
    ("hug", "🤗"),
    ("think", "🤔"),
    ("wonder", "🤔"),
    ("sad", "🙁"),
    ("cry", "😢"),
    ("sweat", "😓"),
    ("whew", "😅"),
    ("worry", "😟"),
    ("confused", "😕"),
    ("surprised", "😮"),
    ("speechless", "😐"),
    ("dull", "😑"),
    ("envy", "😒"),
    ("unamused", "😒"),
    ("sleepy", "😪"),
    ("yawn", "🥱"),
    ("puke", "🤮"),
    ("drunk", "🥴"),
    ("lipssealed", "🤐"),
    ("angry", "😠"),
    ("swear", "🤬"),
    ("devil", "😈"),
    ("evilgrin", "😈"),
    ("doh", "🤦"),
    ("facepalm", "🤦"),
    ("wasntme", "🤷"),
    ("shrug", "🤷"),
    ("bow", "🙇"),
    ("talk", "🗣️"),
    ("hi", "👋"),
    ("wave", "👋"),
    ("bye", "👋"),
    ("y", "👍"),
    ("yes", "👍"),
    ("like", "👍"),
    ("n", "👎"),
    ("no", "👎"),
    ("ok", "👌"),
    ("clap", "👏"),
    ("handshake", "🤝"),
    ("fistbump", "👊"),
    ("fingerscrossed", "🤞"),
    ("rock", "🤘"),
    ("muscle", "💪"),
    ("wait", "✋"),
    ("dance", "💃"),
    ("ninja", "🥷"),
    ("makeup", "💄"),
    ("heart", "❤️"),
    ("brokenheart", "💔"),
    ("star", "⭐"),
    ("fire", "🔥"),
    ("idea", "💡"),
    ("celebrate", "🎉"),
    ("gift", "🎁"),
    ("cake", "🎂"),
    ("champagne", "🍾"),
    ("beer", "🍺"),
    ("drink", "🍸"),
    ("coffee", "☕"),
    ("pizza", "🍕"),
    ("banana", "🍌"),
    ("cash", "💵"),
    ("mail", "✉️"),
    ("call", "📞"),
    ("phone", "📱"),
    ("time", "⏰"),
    ("music", "🎵"),
    ("movie", "🎬"),
    ("games", "🎮"),
    ("smoke", "🚬"),
    ("flower", "🌹"),
    ("sun", "☀️"),
    ("rain", "🌧️"),
    ("rainbow", "🌈"),
    ("umbrella", "☂️"),
    ("island", "🏝️"),
    ("plane", "✈️"),
    ("car", "🚗"),
    ("bike", "🚲"),
    ("xmastree", "🎄"),
    ("bug", "🐛"),
    ("dog", "🐶"),
    ("cat", "🐱"),
    ("monkey", "🐵"),
    ("pig", "🐷"),
    ("sheep", "🐑"),
    ("penguin", "🐧"),
    ("snail", "🐌"),
    ("turkey", "🦃"),
    ("heidy", "🐿️"),
    ("ghost", "👻"),
    ("skull", "💀"),
    ("poop", "💩"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_emoji_is_known() {
        for (code, emoji) in TEAMS_CODES {
            assert!(emojis::get(emoji).is_some(), "({code}) maps to unknown {emoji}");
        }
    }

    #[test]
    fn codes_are_unique() {
        let mut codes: Vec<_> = TEAMS_CODES.iter().map(|(code, _)| code).collect();
        codes.sort();
        let count = codes.len();
        codes.dedup();
        assert_eq!(codes.len(), count);
    }

    #[test]
    fn codes_are_single_words() {
        for (code, _) in TEAMS_CODES {
            assert!(code.chars().all(|c| c.is_ascii_alphanumeric()), "({code})");
        }
    }
}
```

Add `mod teams;` below `mod settings;` in `server/src/main.rs`.

- [ ] **Step 2: Run the table tests**

Run: `cargo test -p emoji-language-server teams::`
Expected: 3 pass. Every emoji in the table was checked against `emojis` 0.9 when this plan was written.

- [ ] **Step 3: Write the failing matching tests in `server/src/matching.rs`**

```rust
    #[test]
    fn test_teams_code_matches_first() {
        let results = find_teams_emojis("laugh", SkinTone::Default);
        assert_eq!(results[0].emoji_char, "😄");
        assert_eq!(results[0].teams_code, Some("laugh"));
    }

    #[test]
    fn test_teams_lists_names_after_codes() {
        let results = find_teams_emojis("smile", SkinTone::Default);
        let first_name_match = results
            .iter()
            .position(|e| e.teams_code.is_none())
            .expect("should list name matches");
        assert!(results[..first_name_match].iter().all(|e| e.teams_code.is_some()));
        assert!(results.iter().any(|e| e.shortcode.as_deref() == Some("smile")));
    }

    #[test]
    fn test_teams_lists_each_emoji_once() {
        let results = find_teams_emojis("y", SkinTone::Default);
        let thumbs_up = results.iter().filter(|e| e.name == "thumbs up").count();
        assert_eq!(thumbs_up, 1);
    }

    #[test]
    fn test_teams_applies_skin_tone() {
        let results = find_teams_emojis("like", SkinTone::Dark);
        assert_eq!(results[0].emoji_char, "👍🏿");
    }

    #[test]
    fn test_teams_results_limited_to_100() {
        assert!(find_teams_emojis("e", SkinTone::Default).len() <= 100);
    }
```

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test -p emoji-language-server matching::`
Expected: compile errors, `cannot find function find_teams_emojis` and `no field teams_code`.

- [ ] **Step 5: Implement `find_teams_emojis`**

Add `pub teams_code: Option<&'static str>,` to `ScoredEmoji` after `shortcode`, and `teams_code: None,` to the `ScoredEmoji` literal in `find_matching_emojis`.

Add `use crate::teams::TEAMS_CODES;` to the imports.

Delete the `let pattern = Pattern::new(...)` statement, its comment and the `if pattern.atoms.is_empty()` check from `find_matching_emojis`, and add:

```rust
// Each word of the query is matched separately, so `thumbs up` matches `thumbsup`.
fn query_pattern(query: &str) -> Option<Pattern> {
    let pattern = Pattern::new(
        query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );
    (!pattern.atoms.is_empty()).then_some(pattern)
}
```

and start `find_matching_emojis` with:

```rust
    let Some(pattern) = query_pattern(query) else {
        return vec![];
    };
```

Add after `find_matching_emojis`:

```rust
/// Finds emojis whose Teams code matches the query, followed by emojis whose name or shortcode
/// matches, as Teams itself lists both.
pub fn find_teams_emojis(query: &str, skin_tone: SkinTone) -> Vec<ScoredEmoji> {
    let Some(pattern) = query_pattern(query) else {
        return vec![];
    };
    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut haystack_buf = vec![];
    let mut results: Vec<ScoredEmoji> = Vec::new();

    for &(code, emoji_str) in TEAMS_CODES {
        haystack_buf.clear();
        let Some(score) = pattern.score(Utf32Str::new(code, &mut haystack_buf), &mut matcher)
        else {
            continue;
        };
        let emoji = emojis::get(emoji_str).expect("Teams codes only map to known emojis");
        // Several codes share an emoji, and the best matching code is the one shown.
        if let Some(existing) = results.iter_mut().find(|e| e.name == emoji.name()) {
            if score > existing.score {
                existing.score = score;
                existing.teams_code = Some(code);
            }
            continue;
        }
        let toned = emoji.with_skin_tone(skin_tone).unwrap_or(emoji);
        results.push(ScoredEmoji {
            emoji_char: toned.as_str().to_string(),
            name: emoji.name().to_string(),
            shortcode: emoji.shortcode().map(|s| s.to_string()),
            teams_code: Some(code),
            score,
        });
    }
    results.sort_by_key(|e| std::cmp::Reverse(e.score));

    for scored in find_matching_emojis(query, skin_tone) {
        if !results.iter().any(|e| e.name == scored.name) {
            results.push(scored);
        }
    }
    results.truncate(100);
    results
}
```

- [ ] **Step 6: Run the matching tests**

Run: `cargo test -p emoji-language-server matching::`
Expected: all pass.

- [ ] **Step 7: Write the failing completion tests in `server/src/main.rs`**

```rust
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
```

- [ ] **Step 8: Run the tests to verify they fail**

Run: `cargo test -p emoji-language-server teams_`
Expected: all five fail, because Teams returns no items yet (`expected an item labeled ...` or `expected an item`).

- [ ] **Step 9: Implement Teams items in `server/src/main.rs`**

Add `find_teams_emojis` to the `use matching::...` import.

In `handle_completion`, replace `end: position,` in the `range` literal with `end,`, and add before `let range`:

```rust
    let mut end = position;
    // Zed closes `(` as it is typed, so a `)` right after the cursor would be left behind the emoji.
    if style == Style::Teams && line_text[byte_offset..].starts_with(')') {
        end.character += 1;
    }
```

Replace the `Style::Teams => Vec::new(),` arm with `Style::Teams => teams_items(query, settings.skin_tone, range),` and add:

```rust
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
```

In `completion_item`, replace the `filter_text` computation with:

```rust
    // Zed filters the items again as the user types, so a Teams code like `y` must be in the text it filters on.
    let code = scored.shortcode.as_deref().unwrap_or(&scored.name);
    let filter_text = match scored.teams_code {
        Some(teams_code) => format!("{teams_code} {code} {}", scored.name),
        None => format!("{code} {}", scored.name),
    };
```

In `main`, change the trigger characters to `Some(vec![":".to_string(), " ".to_string(), "(".to_string()])`. Task 4 makes `(` conditional.

- [ ] **Step 10: Run all checks**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all pass.

- [ ] **Step 11: Commit**

```bash
git add server/src/teams.rs server/src/matching.rs server/src/main.rs
git commit -m "Add Teams codes as a completion style"
```

---

### Task 4: Register `(` as a trigger only while Teams is on

**Files:**
- Modify: `server/src/main.rs` (`main`, new `server_capabilities`, `supports_dynamic_completion`, `teams_trigger_request`, `main_loop`, tests)

**Interfaces:**
- Consumes: `Settings::has`, `Style::Teams`, and the test helpers `start_server`, `notify`, `request` and `did_open` from Task 1.
- Produces:
  - `fn main_loop(connection: Connection, dynamic_registration: bool) -> Result<(), Box<dyn Error + Sync + Send>>`
  - `fn server_capabilities(teams_trigger: bool) -> ServerCapabilities`
  - `fn supports_dynamic_completion(initialize_params: &serde_json::Value) -> bool`
  - `fn teams_trigger_request(id: RequestId, register: bool) -> Request`

- [ ] **Step 1: Write the failing tests**

Change the `start_server` helper to take the flag:

```rust
    fn start_server(dynamic_registration: bool) -> (Connection, ServerThread) {
        let (server, client) = Connection::memory();
        (
            client,
            std::thread::spawn(move || main_loop(server, dynamic_registration)),
        )
    }
```

Change `start_server()` in `configuration_changes_apply_to_later_completions` to `start_server(false)`, and `main_loop(server)` in `answers_bad_requests_and_keeps_running` to `main_loop(server, false)`.

Add a helper and tests:

```rust
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
        assert_eq!(req.params["unregisterations"][0]["id"], TEAMS_REGISTRATION_ID);

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
        assert!(!supports_dynamic_completion(&serde_json::json!({ "capabilities": {} })));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p emoji-language-server`
Expected: compile errors for `main_loop` arguments, `server_capabilities`, `supports_dynamic_completion` and `TEAMS_REGISTRATION_ID`.

- [ ] **Step 3: Implement**

Add to the imports: `use lsp_server::{Request, RequestId};` (merge into the existing `lsp_server` import), and add `CompletionRegistrationOptions, Registration, RegistrationParams, TextDocumentRegistrationOptions, Unregistration, UnregistrationParams` to the `lsp_types` import.

Replace `main` with:

```rust
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
```

Change `main_loop`:

```rust
fn main_loop(
    connection: Connection,
    dynamic_registration: bool,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    let mut documents: HashMap<String, String> = HashMap::new();
    let mut settings = Settings::default();
    let mut teams_registered = false;
    let mut next_request_id = 0;
```

Replace the `Message::Response(_) => {}` arm with:

```rust
            // The server only sends trigger registrations, whose replies need no action.
            Message::Response(_) => {}
```

and the `Ok(params) => settings = Settings::from_json(&params.settings),` line with:

```rust
                    Ok(params) => {
                        settings = Settings::from_json(&params.settings);
                        let teams = settings.has(Style::Teams);
                        if dynamic_registration && teams != teams_registered {
                            next_request_id += 1;
                            let req = teams_trigger_request(RequestId::from(next_request_id), teams);
                            connection.sender.send(Message::Request(req))?;
                            teams_registered = teams;
                        }
                    }
```

- [ ] **Step 4: Run all checks**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add server/src/main.rs
git commit -m "Register the Teams trigger character only while Teams is on"
```

---

### Task 5: Docs, and the manual check in Zed

**Files:**
- Modify: `README.md`
- Modify: `extension.toml` (`description`)

- [ ] **Step 1: Add a "Styles" section to `README.md`, before "Skin tone"**

````markdown
## Styles

Three ways of writing emojis are supported, and any of them can be turned on at the same time:

| Style | Typed | Inserts |
|---|---|---|
| `slack` (on by default) | `:smi` | 😄 |
| `emojisense` | `::smi` | `:smile:` |
| `teams` | `(lau` | 😆 |

Teams style matches the classic Teams codes, such as `(y)` and `(laugh)`, as well as emoji names.

Choose the styles in your Zed `settings.json`:

```json
"lsp": {
  "emoji-language-server": {
    "settings": { "styles": ["slack", "teams"] }
  }
}
```

An empty list turns completions off. Turning on `emojisense` makes `::` at the start of a word trigger completions, as in C++'s `::std::vector`, and turning on `teams` does the same for `(`, as in `if (x`.
````

Remove the line `- Enable emoji markup support similar to emojisense (e.g. `::smile` inserts `:smile:`)` from "Possible improvements".

- [ ] **Step 2: Update `extension.toml`**

```toml
description = "Adds emoji completions when typing ':', '::' or '(' followed by an emoji name."
```

- [ ] **Step 3: Run all checks, including the extension build CI runs**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo build --release --target wasm32-wasip2`
Expected: all pass. If the `wasm32-wasip2` target is not installed, run `rustup target add wasm32-wasip2` first.

- [ ] **Step 4: Commit**

```bash
git add README.md extension.toml
git commit -m "Document emoji styles"
```

- [ ] **Step 5: Manual check in Zed (done by the user)**

Build with `cargo build --release -p emoji-language-server`, point `lsp.emoji-language-server.binary.path` at it, install the dev extension and restart the language server. Then check:

1. With no `styles` setting, `:smi` completes and `::smi` and `(lau` do not.
2. Set `"styles": ["slack", "emojisense", "teams"]` and save without restarting: `::smi` gives `:smile:`, and `(lau` gives 😆 with no `)` left behind.
3. Each item appears once in the menu.
4. Change `skin_tone` to `dark` and save: `(y` gives 👍🏿.
5. Remove `teams` and save: typing `(` in a Rust file no longer opens a completion menu.
