---
model = "claude-opus-4-5"
created = "2026-01-31"
modified = "2026-01-31"
driver = "Isaac Clayton"
source = "https://jsonjoy.com/blog/collaborative-text-sync-plain-text"
---

# json-joy: Collaborative Text Editors (Part 2) - Plain Text Synchronization

## Overview

This is the second post of a four-part series on integrating text editors with json-joy JSON CRDT models. It covers all aspects of plain text synchronization between CRDT models and UI editors.

## Core Architecture

All solutions work by synchronizing the text in the json-joy CRDT model with the text in the UI editor. The architecture involves:

1. **CRDT Model**: json-joy `str` node holding the authoritative text
2. **UI Editor**: Native DOM elements or rich editor libraries
3. **Binding Layer**: Synchronization logic keeping both in sync

## Integration Packages

### collaborative-editor

The core package providing a common interface for integrating with various text editors. Powers all specific integrations.

### collaborative-input

Provides json-joy `str` node synchronization with native web elements:
- `<input>` elements
- `<textarea>` elements

Binds a json-joy `str` node to the DOM element such that text in the model and text in the DOM element are always in sync.

### collaborative-codemirror

For CodeMirror editor integration:
- Three-step process to synchronize a JSON CRDT `str` node with CodeMirror
- Get reference to CodeMirror editor instance
- Call `bind` function from the package
- Pass both references to establish sync

### collaborative-monaco

For Monaco editor integration:
- Same functionality as `collaborative-codemirror`
- React.js users get a wrapper component: `<CollaborativeMonaco>`
- Component accepts a `str` prop (function returning JSON CRDT `str` node)

### collaborative-ace

For Ace editor integration:
- Similar binding approach to other editors

## Synchronization Approach

### Bidirectional Binding

The binding layer handles:
1. **Model -> Editor**: CRDT changes propagate to editor UI
2. **Editor -> Model**: User edits in editor propagate to CRDT

### Change Detection

json-joy implements a fast text diff algorithm for detecting changes:
- Compares previous text state to current
- Generates minimal set of operations
- Applies operations to CRDT model

### Fast-Path Optimizations

Special handling for common editing patterns:
- Single character insertion (most common)
- Backspace deletion
- Forward delete
- Cut/paste of selections

## Key Technical Insights

### 1. Diff Algorithm Performance

json-joy's diff algorithm is optimized for typical editing:
- Sequential typing: O(1) - detect appended character
- Small edits: Fast path for single char changes
- Large pastes: Full diff only when necessary

### 2. State Synchronization

Keeping CRDT model and editor in sync requires:
- Preventing infinite loops (model change -> editor change -> model change)
- Handling concurrent updates from network
- Managing selection/cursor state

### 3. Editor Abstraction

The `collaborative-editor` package abstracts editor differences:
- Consistent API across CodeMirror, Monaco, Ace
- Single integration point for CRDT sync
- Handles editor-specific quirks internally

## Integration Patterns

### Basic Pattern

```javascript
// 1. Get CRDT str node reference
const strNode = model.api.str(['text']);

// 2. Get editor instance reference
const editor = createEditor();

// 3. Bind them together
bind(strNode, editor);

// Done - they stay in sync automatically
```

### React Pattern (Monaco)

```jsx
<CollaborativeMonaco
  str={() => model.api.str(['text'])}
  // other monaco props
/>
```

## Implications for Together

### Editor Integration Architecture

If Together needs editor integration:
1. **Abstraction layer**: Create common interface for different editors
2. **Binding package**: Separate sync logic from CRDT core
3. **Fast paths**: Optimize for common editing patterns

### Diff Algorithm Considerations

json-joy's approach:
- Fast text diff for change detection
- Specialized fast paths for common cases

Our approach could:
- Leverage existing span structure for diff
- Track cursor position to optimize sequential edits
- Use RgaBuf's buffering for batching small edits

### Key Differences

json-joy targets JavaScript/browser environment with:
- DOM element binding
- Rich editor library integration
- React component wrappers

Together is Rust-native, which means:
- Different integration points (potentially via FFI or WASM)
- No DOM concerns
- Focus on core algorithm performance

## Performance Considerations

### Binding Overhead

The synchronization layer adds overhead:
- Diff computation on each change
- Event listener management
- State comparison

json-joy optimizes this with:
- Fast diff algorithm
- Debouncing/batching of changes
- Cursor position caching

### Network Sync

Beyond local editor sync, collaborative editing requires:
- Broadcasting changes to other peers
- Receiving and applying remote changes
- Handling merge conflicts

## References

- Source: https://jsonjoy.com/blog/collaborative-text-sync-plain-text
- CodeMirror: https://codemirror.net/
- Monaco: https://microsoft.github.io/monaco-editor/
- Ace: https://ace.c9.io/
