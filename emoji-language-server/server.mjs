import {
  createConnection,
  TextDocuments,
  ProposedFeatures,
  TextDocumentSyncKind,
  CompletionItemKind,
} from 'vscode-languageserver';

import { TextDocument } from 'vscode-languageserver-textdocument';
import emojiData from 'emojibase-data/en/data.json' with { type: 'json' };

const connection = createConnection(ProposedFeatures.all, process.stdin, process.stdout);

const documents = new TextDocuments(TextDocument);

connection.onInitialize((params) => {
  connection.console.log(`Loaded ${emojiData.length} emojis from emojibase`);

  return {
    capabilities: {
      textDocumentSync: TextDocumentSyncKind.Incremental,
      completionProvider: {
        resolveProvider: false,
        triggerCharacters: [':'],
      },
    },
  };
});

connection.onCompletion((textDocumentPosition) => {
  const document = documents.get(textDocumentPosition.textDocument.uri);
  if (!document) {
    return [];
  }

  const text = document.getText();
  const offset = document.offsetAt(textDocumentPosition.position);

  // Find the start of the emoji trigger (the colon)
  let start = offset - 1;
  while (start >= 0 && text[start] !== ':') {
    start--;
  }

  // If we didn't find a colon, or it's too far back, return no completions
  if (start < 0 || offset - start > 50) {
    return [];
  }

  // Get the text after the colon
  const query = text.substring(start + 1, offset).toLowerCase();

  // Only return emojis that match the query
  if (!query) {
    return [];
  }

  const filteredEmojis = emojiData.filter((item) => {
    const matchesLabel = item.label.toLowerCase().includes(query);
    const matchesTags = item.tags && item.tags.some((tag) => tag.toLowerCase().includes(query));
    const matchesShortcodes = item.shortcodes && item.shortcodes.some((code) => code.toLowerCase().includes(query));
    return matchesLabel || matchesTags || matchesShortcodes;
  });

  // Convert to completion items
  return filteredEmojis.map((item, index) => ({
    label: `:${item.shortcodes?.[0] || item.label}: ${item.emoji}`,
    kind: CompletionItemKind.Text,
    detail: item.label,
    insertText: item.emoji,
    filterText: item.label,
    sortText: `${(item.order || 999999).toString().padStart(6, '0')}`,
    // Replace from the colon to current position
    textEdit: {
      range: {
        start: document.positionAt(start),
        end: textDocumentPosition.position,
      },
      newText: item.emoji,
    },
  }));
});

// Make the text document manager listen on the connection
documents.listen(connection);

// Listen on the connection
connection.listen();

connection.console.log('Emoji Language Server started');
