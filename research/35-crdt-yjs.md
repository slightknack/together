+++
model = "claude-opus-4-5"
created = 2026-02-01
modified = 2026-02-01
driver = "Isaac Clayton"
+++

# Yjs Deep Dive

## Overview

- Repository: https://github.com/yjs/yjs
- Language: JavaScript
- Author: Kevin Jahns
- Primary innovations: YATA algorithm with dual origins, run-length encoding of Items, search markers for O(1) position lookups

Yjs is widely considered the fastest and most memory-efficient CRDT implementation in JavaScript. Joseph Gentle's benchmarks show it is 300x faster than Automerge and uses only 3.3 MB of RAM for 260,000 edits compared to Automerge's 880 MB. Kevin Jahns reportedly rewrote parts of yjs 12 times to achieve this performance.

The YATA (Yet Another Transformation Approach) algorithm was presented in the paper "Near Real-Time Peer-to-Peer Shared Editing on Extensible Data Types" at GROUP 2016.

## Data Structure

### Core Types

Yjs structures its document as a hierarchy of types:

1. **Doc** (`src/utils/Doc.js`): The root container holding all shared types and the StructStore
2. **YType** (`src/ytype.js`): Abstract base for all CRDT types (YText, YArray, YMap)
3. **Item** (`src/structs/Item.js`): The fundamental unit representing inserted content
4. **AbstractStruct** (`src/structs/AbstractStruct.js`): Base class for Item, GC, and Skip
5. **Content types**: ContentString, ContentDeleted, ContentType, ContentAny, etc.

The Item class is the heart of yjs. Each Item has:

```javascript
class Item extends AbstractStruct {
  constructor(id, left, origin, right, rightOrigin, parent, parentSub, content) {
    this.id = id              // ID: { client: number, clock: number }
    this.origin = origin      // ID | null - left neighbor at insertion time
    this.left = left          // Item | null - current left neighbor
    this.right = right        // Item | null - current right neighbor
    this.rightOrigin = rightOrigin  // ID | null - right neighbor at insertion time
    this.parent = parent      // YType | null
    this.parentSub = parentSub // string | null - for map entries
    this.content = content    // AbstractContent
    this.info = 0             // bit flags: keep, countable, deleted, marker
    this.redone = null        // ID | null - for undo/redo
  }
}
```

The `info` field uses bit flags:
- BIT1: keep (prevent garbage collection)
- BIT2: countable (affects visible length)
- BIT3: deleted (tombstone marker)
- BIT4: marker (search marker flag)

### ID Structure

Every operation is identified by a Lamport-style ID:

```javascript
class ID {
  constructor(client, clock) {
    this.client = client  // 53-bit random integer (fits in JS safe integer range)
    this.clock = clock    // monotonically increasing per client
  }
}
```

The clock increments for each character inserted. If a user types "ABC", three IDs are created: `{client: 1, clock: 0}`, `{client: 1, clock: 1}`, `{client: 1, clock: 2}`.

### Memory Layout

Items form a doubly-linked list within their parent YType:

```
YType._start -> Item <-> Item <-> Item <-> Item -> null
                 |                           ^
                 v                           |
              YType._map["key"] -------------+  (for map entries)
```

Each YType maintains:
- `_start`: First Item in the sequence
- `_map`: Map from keys to Items (for YMap-style access)
- `_length`: Cached visible length
- `_searchMarker`: Array of cached position markers

### The StructStore

The StructStore (`src/utils/StructStore.js`) provides fast ID-to-Item lookup:

```javascript
class StructStore {
  constructor() {
    this.clients = new Map()  // Map<clientID, Array<Item|GC>>
    this.pendingStructs = null
    this.pendingDs = null
    this.skips = createIdSet()
  }
}
```

Items are stored per-client in chronological order (by clock). This enables:
- Binary search for ID lookup: O(log n)
- Efficient sync by computing missing operations from state vectors
- Gap detection for out-of-order delivery

The `findIndexSS` function uses binary search with a pivot heuristic:

```javascript
const findIndexSS = (structs, clock) => {
  let left = 0
  let right = structs.length - 1
  let mid = structs[right]
  let midclock = mid.id.clock
  // Pivot search: estimate position based on clock ratio
  let midindex = Math.floor((clock / (midclock + mid.length - 1)) * right)
  while (left <= right) {
    mid = structs[midindex]
    midclock = mid.id.clock
    if (midclock <= clock && clock < midclock + mid.length) {
      return midindex
    }
    // Standard binary search...
  }
}
```

## The YATA Algorithm

### Ordering Rules

YATA determines ordering when concurrent inserts occur at the same position. The algorithm is implemented in `Item.integrate()`:

```javascript
integrate(transaction, offset) {
  // Handle offset for split items
  if (offset > 0) {
    this.id.clock += offset
    this.left = getItemCleanEnd(transaction, ...)
    this.origin = this.left.lastId
    this.content = this.content.splice(offset)
    this.length -= offset
  }

  if (this.parent) {
    // Check if position needs conflict resolution
    if ((!this.left && (!this.right || this.right.left !== null)) || 
        (this.left && this.left.right !== this.right)) {
      
      let left = this.left
      let o  // 'o' is the scanning cursor
      
      // Set o to first potentially conflicting item
      if (left !== null) {
        o = left.right
      } else if (this.parentSub !== null) {
        o = this.parent._map.get(this.parentSub) || null
        while (o !== null && o.left !== null) o = o.left
      } else {
        o = this.parent._start
      }

      const conflictingItems = new Set()
      const itemsBeforeOrigin = new Set()
      
      // Scan through potential conflicts
      while (o !== null && o !== this.right) {
        itemsBeforeOrigin.add(o)
        conflictingItems.add(o)
        
        if (compareIDs(this.origin, o.origin)) {
          // Case 1: Same left origin
          if (o.id.client < this.id.client) {
            // Lower client ID wins - this goes after o
            left = o
            conflictingItems.clear()
          } else if (compareIDs(this.rightOrigin, o.rightOrigin)) {
            // Same origins and higher client - this wins, break
            break
          }
        } else if (o.origin !== null && 
                   itemsBeforeOrigin.has(getItem(store, o.origin))) {
          // Case 2: o's origin is before this's origin
          if (!conflictingItems.has(getItem(store, o.origin))) {
            left = o
            conflictingItems.clear()
          }
        } else {
          break
        }
        o = o.right
      }
      this.left = left
    }
    
    // Reconnect linked list
    if (this.left !== null) {
      const right = this.left.right
      this.right = right
      this.left.right = this
    } else {
      // Insert at start
      let r = this.parent._start
      this.parent._start = this
      this.right = r
    }
    if (this.right !== null) {
      this.right.left = this
    }
    // ... update parent maps, length, etc.
  }
}
```

### Left/Right Origin

The key innovation of YATA over RGA is dual origins. Each Item stores:
- `origin`: The ID of the item that was to its left when it was created
- `rightOrigin`: The ID of the item that was to its right when it was created

This dual-origin approach solves the interleaving problem. Consider:

```
Initial: A . B    (user wants to insert between A and B)
User 1 inserts: X (origin=A, rightOrigin=B)
User 2 inserts: Y (origin=A, rightOrigin=B)

Without rightOrigin, we might get: A Y X B  or  A X Y B
With rightOrigin, YATA guarantees consistent ordering based on client IDs
```

The rightOrigin creates a "boundary" that limits how far the conflict resolution scan needs to go.

### Conflict Resolution

The algorithm has two main cases:

**Case 1: Same origin (left neighbor)**

When two items have the same left origin, they are direct conflicts. The lower client ID goes first:

```javascript
if (compareIDs(this.origin, o.origin)) {
  if (o.id.client < this.id.client) {
    // o has lower client ID, so o goes first
    // this will be inserted after o
    left = o
    conflictingItems.clear()
  } else if (compareIDs(this.rightOrigin, o.rightOrigin)) {
    // Same origins but this has higher priority (comes before o)
    break
  }
}
```

**Case 2: Origin chain**

When items have different origins, we need to check if one item's origin is "before" another's in the document. An item's origin being in the `itemsBeforeOrigin` set means it was created relative to content that precedes the current conflict window:

```javascript
else if (o.origin !== null && itemsBeforeOrigin.has(getItem(store, o.origin))) {
  // o was inserted relative to something before this conflict window
  if (!conflictingItems.has(getItem(store, o.origin))) {
    // o's origin is not in the current conflict set, so o goes first
    left = o
    conflictingItems.clear()
  }
}
```

### Complexity Analysis

**Time Complexity:**
- Local insert: O(1) amortized with search markers, O(n) worst case
- Remote insert (integration): O(c) where c is the number of concurrent inserts at the same position
- Merge: O(n log n) for n operations (binary search per operation)
- Position lookup: O(1) with search markers, O(n) worst case

**Space Complexity:**
- Per character: ~80 bytes minimum in JavaScript (object overhead)
- With run-length encoding: amortized to ~10-20 bytes per character for typical editing
- Delete set: O(d) where d is number of deleted ranges

The real-world benchmark (Martin Kleppmann's editing trace) shows:
- 260,315 operations
- 10,971 Items created (24x compression from run-length encoding)
- 19.7 MB memory usage
- 159,927 bytes encoded document size

## Optimizations

### Item Merging (Run-Length Encoding)

Consecutive characters from the same user are merged into a single Item. The `mergeWith` function:

```javascript
mergeWith(right) {
  if (
    this.constructor === right.constructor &&
    compareIDs(right.origin, this.lastId) &&  // right's origin is this's last char
    this.right === right &&
    compareIDs(this.rightOrigin, right.rightOrigin) &&
    this.id.client === right.id.client &&
    this.id.clock + this.length === right.id.clock &&
    this.deleted === right.deleted &&
    this.redone === null && right.redone === null &&
    this.content.constructor === right.content.constructor &&
    this.content.mergeWith(right.content)
  ) {
    // Update search markers that point to right
    const searchMarker = this.parent._searchMarker
    if (searchMarker) {
      searchMarker.forEach(marker => {
        if (marker.p === right) {
          marker.p = this
          if (!this.deleted && this.countable) {
            marker.index -= this.length
          }
        }
      })
    }
    // Merge
    this.right = right.right
    if (this.right !== null) {
      this.right.left = this
    }
    this.length += right.length
    return true
  }
  return false
}
```

For ContentString, merging is simple string concatenation:

```javascript
// ContentString.mergeWith
mergeWith(right) {
  this.str += right.str
  return true
}
```

This optimization reduces Item count by 14-24x in typical editing scenarios.

### Search Markers

Search markers cache (position, Item) pairs for fast position-to-Item lookup. Without caching, every position lookup would require O(n) linked list traversal.

```javascript
class ArraySearchMarker {
  constructor(p, index) {
    p.marker = true  // Mark the item as a marker target
    this.p = p       // The Item
    this.index = index  // The position
    this.timestamp = globalSearchMarkerTimestamp++
  }
}
```

Yjs maintains up to 80 markers per YType (`maxSearchMarker = 80`). The `findMarker` function:

1. Finds the closest existing marker to the target position
2. Walks the linked list from there (usually very short)
3. Updates or creates a marker at the new position

```javascript
const findMarker = (yarray, index) => {
  if (yarray._start === null || index === 0 || yarray._searchMarker === null) {
    return null
  }
  
  // Find closest marker
  const marker = yarray._searchMarker.reduce((a, b) => 
    Math.abs(index - a.index) < Math.abs(index - b.index) ? a : b
  )
  
  let p = yarray._start
  let pindex = 0
  if (marker !== null) {
    p = marker.p
    pindex = marker.index
    refreshMarkerTimestamp(marker)
  }
  
  // Walk right if needed
  while (p.right !== null && pindex < index) {
    if (!p.deleted && p.countable) {
      if (index < pindex + p.length) break
      pindex += p.length
    }
    p = p.right
  }
  
  // Walk left if needed
  while (p.left !== null && pindex > index) {
    p = p.left
    if (!p.deleted && p.countable) {
      pindex -= p.length
    }
  }
  
  // Create or update marker
  return markPosition(yarray._searchMarker, p, pindex)
}
```

When changes occur, markers are updated via `updateMarkerChanges`:

```javascript
const updateMarkerChanges = (searchMarker, index, len) => {
  for (let i = searchMarker.length - 1; i >= 0; i--) {
    const m = searchMarker[i]
    if (index < m.index || (len > 0 && index === m.index)) {
      m.index = Math.max(index, m.index + len)
    }
  }
}
```

### Garbage Collection

Yjs uses tombstones for deletions, but can optionally garbage collect deleted content. The GC class (`src/structs/GC.js`) replaces deleted Items:

```javascript
class GC extends AbstractStruct {
  get deleted() { return true }
  
  mergeWith(right) {
    if (this.constructor !== right.constructor) return false
    this.length += right.length
    return true
  }
  
  integrate(transaction, offset) {
    addToIdSet(transaction.deleteSet, this.id.client, this.id.clock, this.length)
    addStruct(transaction.doc.store, this)
  }
}
```

GC items:
- Are much smaller than regular Items (no content, no pointers)
- Can be merged with adjacent GC items
- Preserve the ID space for sync purposes

The GC process in `Transaction.js`:

```javascript
const tryGcDeleteSet = (tr, ds, gcFilter) => {
  for (const [client, deleteItems] of ds.clients.entries()) {
    const structs = tr.doc.store.clients.get(client)
    for (const deleteItem of deleteItems) {
      for (let si = findIndexSS(structs, deleteItem.clock); 
           struct.id.clock < deleteItem.clock + deleteItem.len; 
           struct = structs[++si]) {
        if (struct instanceof Item && struct.deleted && !struct.keep && gcFilter(struct)) {
          struct.gc(tr, false)  // Convert to GC or ContentDeleted
        }
      }
    }
  }
}
```

### Deletion Handling

Deletions are state-based, not operation-based:

```javascript
delete(transaction) {
  if (!this.deleted) {
    const parent = this.parent
    if (this.countable && this.parentSub === null) {
      parent._length -= this.length
    }
    this.markDeleted()
    addToIdSet(transaction.deleteSet, this.id.client, this.id.clock, this.length)
    addChangedTypeToTransaction(transaction, parent, this.parentSub)
    this.content.delete(transaction)
  }
}
```

Key properties:
- No clock increment for deletes
- No operation ID for deletes
- Deletes stored in IdSet (ranges of deleted IDs)
- Much more compact than storing full delete operations

### Transaction Batching

Changes are batched in transactions to minimize observer calls and network messages:

```javascript
const transact = (doc, f, origin = null, local = true) => {
  if (doc._transaction === null) {
    doc._transaction = new Transaction(doc, origin, local)
    doc._transactionCleanups.push(doc._transaction)
    doc.emit('beforeTransaction', [doc._transaction, doc])
  }
  try {
    result = f(doc._transaction)
  } finally {
    if (initialCall) {
      doc._transaction = null
      cleanupTransactions(transactionCleanups, 0)
    }
  }
  return result
}
```

After transaction cleanup:
1. Observer callbacks are fired
2. GC is attempted on deleted items
3. Adjacent items are merged
4. Update messages are encoded and emitted

## Code Walkthrough

### Inserting Text

When a user types at position `pos`:

```javascript
// In YText or similar
insertText(pos, text) {
  transact(this.doc, transaction => {
    const { left, right, index, currentAttributes } = 
      this._findPosition(pos, transaction)
    
    const content = new ContentString(text)
    const item = new Item(
      createID(doc.clientID, getState(doc.store, doc.clientID)),
      left,
      left && left.lastId,  // origin
      right,
      right && right.id,    // rightOrigin
      this,                 // parent
      null,                 // parentSub
      content
    )
    item.integrate(transaction, 0)
  })
}
```

### Applying Remote Updates

Remote updates are decoded and applied:

```javascript
// Simplified from updates.js
const applyUpdate = (ydoc, update) => {
  const decoder = new UpdateDecoder(update)
  transact(ydoc, transaction => {
    // Read structs
    const clientsCount = decoder.readVarUint()
    for (let i = 0; i < clientsCount; i++) {
      const client = decoder.readClient()
      const clock = decoder.readVarUint()
      const count = decoder.readVarUint()
      
      for (let j = 0; j < count; j++) {
        const struct = readStruct(decoder)
        // Check for missing dependencies
        const missing = struct.getMissing(transaction, store)
        if (missing !== null) {
          // Queue for later
          addPending(struct)
        } else {
          struct.integrate(transaction, 0)
        }
      }
    }
    
    // Read and apply delete set
    const ds = readIdSet(decoder)
    applyDeleteSet(transaction, ds)
  })
}
```

### Sync Protocol (Simplified)

State vector exchange for sync:

```javascript
// Step 1: Send state vector
const sv = getStateVector(doc.store)  // Map<client, clock>
send(encodeStateVector(sv))

// Step 2: Receive state vector, compute and send diff
const remoteSV = decodeStateVector(message)
const update = encodeStateAsUpdate(doc, remoteSV)
send(update)

// Step 3: Receive and apply update
const update = receive()
applyUpdate(doc, update)
```

## Lessons for Our Implementation

### What to Adopt

1. **Dual origins (left + right)**: Essential for preventing interleaving. The rightOrigin creates a boundary for conflict resolution.

2. **Run-length encoding**: Critical for performance. Consecutive inserts from one user should be a single struct, not individual items.

3. **Search markers**: Cache position lookups. Users edit sequentially, so the last few edit positions are likely to be near future edits.

4. **State-based deletions**: Deletions as tombstone flags with IdSet ranges is more compact than storing delete operations.

5. **Binary search in StructStore**: Store items per-client in clock order for O(log n) ID lookup.

6. **Transaction batching**: Group changes to minimize overhead from observer calls and merge attempts.

7. **Lazy GC**: Do not GC immediately. Batch it at transaction end and allow filtering.

### What to Consider Differently

1. **JavaScript-specific optimizations**: Many yjs optimizations are JavaScript-specific (avoiding object allocation, V8-friendly patterns). In Rust, different optimizations apply (cache-friendly memory layout, arena allocation).

2. **No delete timestamps**: Yjs does not store when an item was deleted or by whom. This saves memory but prevents per-keystroke replay. Consider if time-travel is needed.

3. **Flat linked list**: Yjs uses a flat doubly-linked list. For very large documents, a tree structure (like diamond-types' JumpRope) might scale better.

4. **Marker cleanup**: Search markers need cleanup when their target items are merged or deleted. This adds complexity.

5. **Content polymorphism**: yjs uses a content type hierarchy. In Rust, an enum might be more cache-friendly than trait objects.

## Sources

- [YATA Paper](https://www.researchgate.net/publication/310212186_Near_Real-Time_Peer-to-Peer_Shared_Editing_on_Extensible_Data_Types) - Kevin Jahns et al., GROUP 2016
- [Kevin's Blog: Are CRDTs suitable for shared editing?](https://blog.kevinjahns.de/are-crdts-suitable-for-shared-editing)
- [CRDTs go brrr](https://josephg.com/blog/crdts-go-brrr/) - Joseph Gentle's performance analysis
- [Delta-state CRDTs: indexed sequences with YATA](https://www.bartoszsypytkowski.com/yata/) - Bartosz Sypytkowski
- [Yjs Internals Documentation](https://docs.yjs.dev/api/internals)
