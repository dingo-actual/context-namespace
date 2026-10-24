# Architecture of context-namespace

## The problem

Long-running LLM agents accumulate history records. Every model call
re-sends the full history, so token cost grows linearly with the
conversation length. Summarization reduces this cost, but flat
summaries lose structure: the agent cannot selectively re-expand the
parts it needs.

## The solution

`context-namespace` stores ordered records and a tree of summary nodes.
Each node has:

- an immutable `NodeId` (UUIDv7)
- a human-readable absolute path (e.g. `/task/investigation-3`)
- summary text and a short gloss
- the compressed source material it was created from
- tree connectivity (parent, children)

The agent can expand a node by path for N renders. During rendering,
expanded nodes show their stored source instead of their collapsed
marker. Canonical state never changes during expansion; the overlay is
read-only.

## Key design decisions

### Generational arena for internal keys

Nodes are stored in a `slotmap::SlotMap<NodeKey, NamespaceNode>`.
`NodeKey` is a generational key: if a node is ever removed and the slot
reused, old keys detect the generation mismatch. External code never
sees `NodeKey`; snapshots serialize `NodeId` (UUID) and rebuild the
arena on restore.

### Two-phase transactional summarization

Summarization is split into prepare and commit:

1. **Prepare** validates the selection, extracts source text, computes
   a content hash, and stores a `PendingSummary` with a
   `TransactionToken`. The token is sealed with SHA-256 over
   (request_id, source_hash, namespace_version).

2. **Commit** revalidates the token, checks that the namespace version
   has not changed, validates the summary result, builds a mutation plan
   off-state, and applies it atomically.

This prevents stale commits when a mutation occurs between prepare and
commit. The pending request is not serialized into snapshots.

### Expansion as a read-only overlay

`expand(path, ttl)` sets a TTL counter on a node. During `render()`:

- Expanded nodes show their stored source text in place of the carrier
  reference (stored records joined with newlines). References found
  inside the inlined source render by the same rules recursively.
- Collapsed nodes show
  `[summary(/path): <summary>; gloss(/child-path): <gloss>, ...]` —
  the node's summary text leading with its current path, followed by
  one path-prefixed gloss per immediate child. The child gloss list is
  included only when `include_child_glosses` is enabled (default).
- After successful serialization, all active TTLs decrement. Nodes at
  TTL zero are collapsed automatically.

Canonical records, node tree, aliases, and hashes are never modified by
expansion or rendering (except TTL decrement).

### Render ID for stale-index safety

Each `render()` allocates a `RenderId` and stores a `RenderResolution`
mapping visible indices to canonical positions. Operations that accept
visible indices (like `prepare_record_summary`) require a `RenderId`.
If the namespace version has changed since that render, the operation
returns `StaleRender`.

### Paths and reparenting

Paths use JSON Pointer escaping (`~` becomes `~0`, `/` becomes `~1`).
When a new summary wraps existing nodes, those nodes are reparented:

1. The new node becomes their parent.
2. Their immutable `NodeId` stays unchanged.
3. Their current paths are recomputed.
4. Old paths enter the alias table.
5. Expansion accepts the current path (`expand`) or the current path
   and node ID (`expand_node`). Retained aliases are not accepted by
   path resolution or expansion.

The alias table is currently unbounded: `max_aliases_per_node` is
declared in `DefensiveLimits` but not yet enforced.

### Bounded event queue

Events are stored in a `VecDeque` with a configurable ceiling
(`max_event_queue`). When the queue is full, the oldest event is
dropped and a `dropped_events` counter increments. This prevents
unbounded memory growth in long-running agents.

### Snapshot integrity

Snapshots are versioned JSON (`NamespaceSnapshotV1`) with a content
SHA-256. Restore:

1. Decodes the envelope with `serde_path_to_error` so decode errors
   report the failing field path.
2. Validates the schema version (`RestoreOptions` can add `min_version`
   and `max_version` bounds).
3. Validates unique record/node IDs.
4. Creates arena nodes and the ID lookup.
5. Resolves parent/child links and rebuilds paths, owners, aliases, and
   expansion state.
6. Runs full invariant validation, and rejects the snapshot on failure.

The stored `content_sha256` is serialized with the snapshot envelope but
restore does not currently re-verify it against the decoded content. If
any step fails, no namespace is returned.

## Module ownership

| Module | Owns |
|---|---|
| `record` | Record validation, render-field split/reconstruction |
| `node` | Node/source structs, child enumeration |
| `path` | Labels, escaping, path/alias derivation |
| `namespace` | Central state machine, all public operations |
| `transaction` | Transaction token, pending state, mutation plans |
| `policy` | Config validation |
| `render` | Rendered output types, preview types |
| `snapshot` | Snapshot envelope, snapshot save, restore |
| `validation` | Full and touched invariant checking |
| `event` | Event and structural-stats types (bounded queue lives in `namespace`) |
| `adapter` | Async `Summarizer` trait (feature-gated) |
| `embedding` | Vector types (feature-gated) |
| `simple` | High-level `SimpleContextNamespace` wrapper |

## Invariants

The namespace maintains 12 invariants, checked by `validate()`:

1. The parent/child graph is a forest: no cycles, at most one parent.
2. Every reachable structural reference points to an existing node.
3. Every node has exactly one canonical owner/reference location.
4. A child listed by a node is present in that node's stored source.
5. Sibling labels are unique after normalized path escaping.
6. Current paths resolve uniquely.
7. Aliases resolve to exactly one live node.
8. Every record has a unique stable ID.
9. The configured render field is represented exactly once by
   `Record.render`.
10. Pending requests do not mutate canonical state.
11. Expansion state references only live nodes with positive TTLs.
12. `version` increases after each canonical mutation, not after
    rendering.
