# Emoji styles: emojisense and Teams

## Goal

Add two ways of writing emojis next to the existing Slack style, each of which can be turned on or off independently:

| Style | Typed | Inserts | Default |
|---|---|---|---|
| Slack (existing) | `:smi` | 😄 | on |
| emojisense | `::smi` | `:smile:` | off |
| Teams | `(lau` | 😆 | off |

Any combination of styles can be on at the same time. Settings apply without restarting the language server.

## Settings

Users configure the server through `lsp.emoji-language-server.settings`:

```json
"lsp": {
  "emoji-language-server": {
    "settings": { "styles": ["slack", "teams"], "skin_tone": "medium-dark" }
  }
}
```

- `styles` is a list containing any of `"slack"`, `"emojisense"` and `"teams"`. A missing `styles` means `["slack"]`. An empty list turns completions off. Unknown entries are logged to stderr and ignored, as unknown `skin_tone` values are today.
- `skin_tone` keeps its current values and default, but moves from `initialization_options` to `settings`. It has not shipped in a release, so `initialization_options` is no longer read.

### How settings reach the server

- `src/lib.rs` implements `language_server_workspace_configuration`, returning `LspSettings::for_worktree("emoji-language-server", worktree)` `.settings`.
- The server keeps a `Settings { styles, skin_tone }` value, which `main_loop` and `handle_completion` take in place of the current `skin_tone` argument. It starts from the defaults.
- After initialization the server sends a `workspace/configuration` request for the section `emoji-language-server`. The reply arrives in the `Message::Response` branch of `main_loop`, which replaces the settings with it.
- On `workspace/didChangeConfiguration` the server replaces the settings with the notification's `settings` payload. If Zed turns out to send an empty payload, the server sends a new `workspace/configuration` request instead.
- During implementation, check in Zed that it answers `workspace/configuration` from the extension's `language_server_workspace_configuration`, and what it puts in `didChangeConfiguration`.

## Triggers

`emoji_query` returns the start position of the replaced text, the style and the query. The characters just before the query decide the style, so at most one style applies at a time. If that style is off, the response is an empty incomplete list.

| Before the query | Style |
|---|---|
| `::`, where the first `:` is at line start or after whitespace or `(` | emojisense |
| `:` at line start or after whitespace or `(` (unchanged) | Slack |
| `(` at line start or after whitespace | Teams |

- The existing two-word query rule applies to all styles.
- `std::fs` and `key:value` still never trigger. `::smi` triggers nothing unless emojisense is on, as today.
- Known false triggers when a style is on: global scope in C++ at the start of a word (`::std::vector`) for emojisense, and `if (x` for Teams. These are documented in the README.

### Trigger characters

- The server declares `:` and space at startup.
- If the client reports `textDocument.completion.dynamicRegistration`, the server sends `client/registerCapability` for `textDocument/completion` with trigger character `(` and a fixed registration id whenever the settings turn Teams on, and `client/unregisterCapability` with that id when they turn it off. It only sends these when Teams' on/off state changes. Replies to these requests are ignored.
- If the client does not report dynamic registration, `(` is declared at startup.
- During implementation, check that Zed does not send one completion request per registration, which would show each item twice. If it does, always declare `(` at startup instead of registering it.

## Matching and inserted text

### Slack

Unchanged: `find_matching_emojis` with the skin tone, inserting the emoji.

### emojisense

- Uses the same matching as Slack, but only emojis with a shortcode are offered, since others have no markup form.
- Inserts `:shortcode:` text, using the shortcode shown in the label. The skin tone is ignored, because shortcodes do not encode it.
- The label is `:smile: 😄`. The replaced range starts at the first `:`.

### Teams

- A new `server/src/teams.rs` holds about 100 classic Skype/Teams codes as `(code, emoji)` pairs, such as `("y", "👍")` and `("laugh", "😆")`. Codes with no Unicode equivalent are left out.
- The query is fuzzy-matched with the same `nucleo-matcher` pattern against the Teams codes and against the normal names and shortcodes.
- Teams code matches come first, ordered by score, followed by name and shortcode matches. Emojis already listed are not repeated, and the total is capped at 100.
- The inserted emoji gets the skin tone where it has one.
- Labels are `(laugh) 😆` for a Teams code, and `(:smile 😄` for a name or shortcode match.
- The replaced range starts at `(`. If the character right after the cursor is `)`, the range also covers it, because Zed auto-closes `(` in most languages.

## Testing

- **`main.rs` unit tests:**
  - trigger detection per style: `::smi` gives emojisense, `(lau` gives Teams, and `foo(bar`, `std::fs` and `key:value` give nothing;
  - inserted text: `:smile:` from `::smi`, and the toned 👍 from `(y` with a dark skin tone;
  - edit ranges, including replacing a `)` right after the cursor;
  - a style that is off returns an empty list.
- **Settings tests**, in the style of the existing `skin_tone` tests: a missing `styles` gives Slack only, unknown entries are ignored, and an empty list turns completions off.
- **`teams.rs` tests:** every code's emoji is known to `emojis::get`, codes are unique, and a Teams code match is listed before name matches.
- **Protocol test**, extending the `Connection::memory()` test:
  - a `didChangeConfiguration` that turns Teams on makes the server send `client/registerCapability`, and turning it off sends `client/unregisterCapability`;
  - a reply to `workspace/configuration` updates the settings;
  - with no dynamic registration in the client capabilities, `(` is among the startup trigger characters.
- **Manual check in Zed** with a dev build:
  - settings apply without a restart;
  - `(` only triggers with Teams on;
  - items are not duplicated;
  - the auto-closed `)` is replaced.

## Docs

- The README skin tone section moves to `settings` and drops the restart step.
- A new README section describes `styles`, the three styles and the known false triggers.
- The emojisense entry is removed from "Possible improvements".
- The `extension.toml` description mentions the `::` and `(` triggers.
