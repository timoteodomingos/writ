# writ

A hybrid markdown editor that seamlessly combines raw text editing with live inline rendering.

## Architecture

The buffer stores raw markdown text using ropey, a rope data structure that provides O(log n) insertions and deletions. On every edit, tree-sitter incrementally reparses the document. Tree-sitter-md produces two parse trees: a block tree representing document structure (paragraphs, headings, lists, code blocks) and separate inline trees for each paragraph's inline content (bold, italic, links). The parser maintains both trees and provides a unified cursor that transparently switches between them when traversing.

Each render frame, the editor extracts line information from the buffer. A line is represented as a stack of layers, where each layer corresponds to a nesting level in the document structure. For example, a task item inside a blockquote produces two layers: `[BlockQuote, ListItem]`. Each layer knows its marker range (the bytes to hide when the cursor is away), its visual substitution (e.g., `-` becomes `•`), and its continuation text for smart enter.

The line view component renders each line independently. It determines whether to show or hide markers based on cursor position: if the cursor is on the line, raw markdown syntax is visible for editing; otherwise, markers are hidden and substitutions are shown. For inline styles like bold or italic, the same logic applies per-span. Click handling maps visual positions back to buffer offsets by accounting for hidden characters.

### Incremental Parsing

Tree-sitter's incremental parsing is central to writ's responsiveness. When you type a character, tree-sitter doesn't reparse the entire document. Instead, the buffer tells tree-sitter what changed (the byte range and new content), and tree-sitter reuses unchanged portions of the previous syntax tree. The complexity is O(log n + k) where n is the document size and k is the size of the change, rather than O(n) for a full reparse. This means editing a 10,000-line document feels the same as editing a 100-line document.

### Code Block Syntax Highlighting

Code blocks are highlighted using tree-sitter-highlight with language-specific grammars. The editor walks the markdown AST to find fenced code blocks, extracts their content along with the language identifier from the fence line, and highlights each block separately using the appropriate grammar.

This manual extraction approach was chosen over tree-sitter's built-in injection support, which proved unreliable for our use case. Editors like Zed and Helix build their own injection handling for similar reasons. The manual approach is simpler: we find code blocks, highlight them independently, and merge the results back with buffer-relative offsets.

Currently only Rust is supported, but adding new languages requires just the grammar crate and a highlights.scm query file. The highlighting runs on every edit, which is fast enough for typical documents since tree-sitter-highlight is efficient. For documents with many large code blocks, caching per-block highlights and invalidating only affected blocks would be a straightforward optimization.
