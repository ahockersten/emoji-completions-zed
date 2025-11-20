#!/usr/bin/env node

const {
  createConnection,
  TextDocuments,
  ProposedFeatures,
  TextDocumentSyncKind,
  CompletionItemKind,
} = require('vscode-languageserver/node');

const { TextDocument } = require('vscode-languageserver-textdocument');

// Create a connection for the server using stdio
const connection = createConnection(ProposedFeatures.all, process.stdin, process.stdout);

// Create a simple text document manager
const documents = new TextDocuments(TextDocument);

// Emoji data - for now just one example as requested
const EMOJI_DATA = [
  { name: 'sad', emoji: '😢', description: 'Crying face' },
  { name: 'sad', emoji: '😞', description: 'Disappointed face' },
  { name: 'sad', emoji: '😔', description: 'Pensive face' },
];

connection.onInitialize((params) => {
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

  // If the query is empty, return all emojis
  // Otherwise filter by emojis that match the query
  const filteredEmojis = EMOJI_DATA.filter((emoji) => {
    return emoji.name.includes(query) || emoji.description.toLowerCase().includes(query);
  });

  // Convert to completion items
  return filteredEmojis.map((emoji, index) => ({
    label: `:${emoji.name}: ${emoji.emoji}`,
    kind: CompletionItemKind.Text,
    detail: emoji.description,
    insertText: emoji.emoji,
    filterText: `:${emoji.name}`,
    sortText: `${index.toString().padStart(5, '0')}`,
    // Replace from the colon to current position
    textEdit: {
      range: {
        start: document.positionAt(start),
        end: textDocumentPosition.position,
      },
      newText: emoji.emoji,
    },
  }));
});

// Make the text document manager listen on the connection
documents.listen(connection);

// Listen on the connection
connection.listen();

connection.console.log('Emoji Language Server started');
