# ContextNamespace API Reference

## Construction

### `ContextNamespace::new`

```rust
pub fn new(config: NamespaceConfig) -> Result<Self, NamespaceError>
```

Creates an empty namespace.

**Errors:** `NamespaceError::Config` if the configuration is invalid.

### `ContextNamespace::from_records`

```rust
pub fn from_records<I>(config: NamespaceConfig, records: I) -> Result<Self, NamespaceError>
where I: IntoIterator<Item = NewRecord>
```

Creates a namespace and appends the given records.

**Errors:** `NamespaceError` if the config is invalid or any record
fails validation.

## Record construction

### `NewRecord::new`

```rust
pub fn new(
    fields: IndexMap<String, serde_json::Value>,
    render_field: &str,
    limits: &DefensiveLimits,
) -> Result<Self, NamespaceError>
```

Creates a validated record from a field map. The render field must be
present and hold a string; it is extracted from `fields` and stored
separately as the record's render text. The reserved metadata key
(`__ctx` by default) must not appear in `fields`.

**Errors:** `MissingRenderField` if the render field is missing or not
a string; `Config` if the reserved metadata key is present;
`LimitExceeded` if the render text exceeds `max_record_bytes`.

**Complexity:** O(record bytes).

### `Segment`

| Variant | Field | Description |
|---|---|---|
| `Text` | `String` | Literal text from the record's render field |
| `Ref` | `NodeId` | Structural reference to a summary node |

Records store their render field as `Vec<Segment>`: initially a single
`Text` segment, replaced by mixed `Text`/`Ref` segments after summary
commits. `Ref` segments occupy zero character width, so a
`TextSelection` range can never split one; a range covering a `Ref`
position makes the referenced node a child of the new summary.

### `Record` accessors

| Method | Return type | Description |
|---|---|---|
| `id()` | `RecordId` | The record's stable ID |
| `fields()` | `&IndexMap<String, Value>` | The non-render fields |
| `render()` | `&[Segment]` | The render segments |
| `render_text()` | `String` | `Text` segments concatenated; `Ref` segments skipped |
| `fields_with_render(field)` | `IndexMap<String, Value>` | Full field map including the reconstructed render field |

## Accessors

### `config`

```rust
pub fn config(&self) -> &NamespaceConfig
```

Returns a reference to the namespace configuration.

### `version`

```rust
pub fn version(&self) -> u64
```

Returns the current namespace version. Increments on every canonical
mutation (append, commit). Does not increment on render.

### `stats`

```rust
pub fn stats(&self) -> StructuralStats
```

Returns structural statistics.

| Field | Type | Description |
|---|---|---|
| `records` | `u32` | Canonical record count |
| `nodes` | `u32` | Summary node count |
| `active_expansions` | `u32` | Active expansion count (TTL > 0) |
| `pending_requests` | `u32` | Pending summary request count |
| `alias_count` | `u32` | Retained path alias count |
| `max_depth` | `u32` | Maximum tree depth (0 = no nodes) |
| `dropped_events` | `u32` | Events dropped due to queue overflow |

## Append

### `append`

```rust
pub fn append(&mut self, record: NewRecord) -> Result<AppendReceipt, NamespaceError>
```

Appends a record with a stable immutable `RecordId`.

**Errors:** `NamespaceError::LimitExceeded` if `max_records` is reached.

**Complexity:** O(1) amortized.

## Rendering

### `render`

```rust
pub fn render(&mut self) -> Result<RenderedHistory, NamespaceError>
```

Produces deterministic JSON with visible indices, stable record IDs, and
expansion overlay. Collapsed references render as
`[summary(/path): <summary>; gloss(/child-path): <gloss>, ...]`
markers (the child gloss list is included only when
`RenderOptions::include_child_glosses` is enabled). Expanded references
inline the referenced node's stored source text (records joined with
newlines), recursively. Allocates a `RenderId` and stores a render
resolution for visible-index validation. Decrements active expansion
TTLs after successful serialization.

**Errors:** `NamespaceError::LimitExceeded` if the rendered output
exceeds `max_render_bytes`.

**Complexity:** O(emitted bytes).

### `RenderedHistory`

| Field | Type | Description |
|---|---|---|
| `render_id` | `RenderId` | Unique ID for this render |
| `namespace_version` | `u64` | Namespace version at render time |
| `json` | `String` | The rendered JSON string |
| `bytes` | `u64` | Byte count of `json` |
| `entries` | `u32` | Number of visible entries |

## Maintenance planning

### `preview_render`

```rust
pub fn preview_render(&self) -> Result<RenderPreview, NamespaceError>
```

Produces a non-mutating render preview with per-record maintenance units.
Unlike `render`, this method does not allocate a `RenderId`, register
visible-index resolutions, emit a render event, or decrement expansion
TTLs. The harness calls this before each model call to estimate per-record
token counts, then passes those estimates to `plan_maintenance`.

**Errors:** `NamespaceError::LimitExceeded` if the rendered output
exceeds `max_render_bytes`.

**Complexity:** O(emitted bytes).

### `plan_maintenance`

```rust
pub fn plan_maintenance(
    &mut self,
    input: MaintenanceInput,
) -> Result<MaintenanceDecision, NamespaceError>
```

Validates harness-supplied per-record token estimates against the current
canonical state (record count, IDs, and render digests must all agree).
If `estimated_next_tokens` meets or exceeds `trigger_tokens`, selects the
oldest eligible consecutive record range — respecting
`preserve_prefix_records` and `preserve_recent_records` — large enough to
target `target_tokens`, and prepares a record-range summary. If the
trigger is not met or no eligible range exists, returns `NoAction`. If the
entire eligible range is smaller than `target_tokens`, the whole
eligible range is selected anyway once the trigger has fired.

This method performs an internal `render` to obtain a fresh `RenderId`
for the prepared selection; that render decrements expansion TTLs and
emits a `Render` event like any other render.

**Errors:** `NamespaceError::PolicyViolation` if the estimates do not
match the current state (wrong record count, stale digest, or
unknown record ID); `LimitExceeded` if a defensive limit is reached
during preparation.

**Complexity:** O(records) for validation plus O(selected range) for
preparation.

### `RenderPreview`

| Field | Type | Description |
|---|---|---|
| `namespace_version` | `u64` | Namespace version at preview time |
| `json` | `String` | The rendered JSON string |
| `bytes` | `u64` | Byte count of `json` |
| `entries` | `u32` | Number of visible entries |
| `maintenance_units` | `Vec<MaintenanceUnit>` | Per-record units for token estimation |

### `MaintenanceUnit`

| Field | Type | Description |
|---|---|---|
| `record_id` | `RecordId` | The stable record ID |
| `render_digest` | `ContentHash` | SHA-256 digest of the rendered JSON fragment |
| `json_fragment` | `String` | The rendered JSON fragment for this record |

### `MaintenanceDecision`

| Variant | Fields | Description |
|---|---|---|
| `Summarize` | `PreparedSummary` | A summary is prepared and ready to commit |
| `NoAction` | `stats: StructuralStats` | No maintenance needed; current stats included |

### `MaintenanceInput`

| Field | Type | Description |
|---|---|---|
| `estimated_next_tokens` | `u32` | Harness estimate of total tokens in the next render |
| `records` | `Vec<RecordTokenEstimate>` | Per-record token estimates with digests |

### `RecordTokenEstimate`

| Field | Type | Description |
|---|---|---|
| `record_id` | `RecordId` | The stable record ID |
| `render_digest` | `ContentHash` | SHA-256 digest of the rendered JSON fragment |
| `estimated_tokens` | `u32` | Harness estimate of tokens for this record |

## Summarization

### `prepare_record_summary`

```rust
pub fn prepare_record_summary(
    &mut self,
    selection: RecordSelection,
    label: NodeLabel,
) -> Result<PreparedSummary, NamespaceError>
```

Validates a consecutive record range and creates a pending summary
request with a hash-bound transaction token.

**Errors:** `StaleRender`, `InvalidSelection`, `LimitExceeded`.

**Complexity:** O(selected source).

### `prepare_text_summary`

```rust
pub fn prepare_text_summary(
    &mut self,
    selection: TextSelection,
    label: NodeLabel,
) -> Result<PreparedSummary, NamespaceError>
```

Validates a Unicode scalar-value character range within one record's
render field and creates a pending summary request. `Ref` segments
occupy zero character width, so a range cannot split one; a range
covering a `Ref` position makes the referenced node a child of the new
summary.

**Errors:** `StaleRender`, `InvalidSelection`, `LimitExceeded`.

**Complexity:** O(record length + children).

### `commit_summary`

```rust
pub fn commit_summary(
    &mut self,
    prepared: PreparedSummary,
    result: SummaryResult,
) -> Result<SummaryReceipt, NamespaceError>
```

Revalidates the transaction token, source hash, and namespace version.
Builds a mutation plan off-state, checks path collisions, and applies
atomically. Increments the namespace version.

**Errors:** `StaleSummary`, `InvalidSelection`, `PathCollision`.

**Complexity:** O(selected source + affected descendants).

### `cancel_summary`

```rust
pub fn cancel_summary(&mut self, request_id: RequestId) -> Result<CancelReceipt, NamespaceError>
```

Removes a pending request without mutating canonical state.

**Errors:** `StaleSummary` if the request ID is not found.

### `RecordSelection`

| Field | Type | Description |
|---|---|---|
| `render_id` | `RenderId` | Fresh render ID for visible-index validation |
| `start_index` | `u32` | Half-open range start (inclusive) |
| `end_index` | `u32` | Half-open range end (exclusive) |

### `TextSelection`

| Field | Type | Description |
|---|---|---|
| `record` | `RecordLocator` | Record by ID or visible index |
| `start_char` | `u32` | Unicode scalar-value offset (inclusive) |
| `end_char` | `u32` | Unicode scalar-value offset (exclusive) |

### `RecordLocator`

| Variant | Fields | Description |
|---|---|---|
| `Id` | `RecordId` | Locate by immutable record ID |
| `Visible` | `render_id: RenderId, index: u32` | Locate by visible index in a specific render |

### `SummaryResult`

| Field | Type | Description |
|---|---|---|
| `summary` | `String` | Summary text from the summarizer |
| `gloss` | `String` | Short gloss for collapsed rendering |

### `SummaryReceipt`

| Field | Type | Description |
|---|---|---|
| `new_node_id` | `NodeId` | ID of the new summary node |
| `carrier_record_id` | `RecordId` | ID of the carrier record (record-range) or edited record (text-slice) |
| `replaced_record_ids` | `Vec<RecordId>` | IDs of replaced canonical records |
| `render_id` | `RenderId` | Render ID associated with the commit |

### `SummaryInput`

| Field | Type | Description |
|---|---|---|
| `request_id` | `RequestId` | The request ID associated with this summary |
| `source` | `String` | The rendered source text extracted from canonical collapsed state |
| `children` | `Vec<ChildDescriptor>` | Immediate child nodes referenced by the selection |
| `max_summary_chars` | `u32` | Effective maximum summary length: the tighter of the policy and defensive limits |
| `max_gloss_chars` | `u32` | Effective maximum gloss length: the tighter of the policy and defensive limits |

### `ChildDescriptor`

| Field | Type | Description |
|---|---|---|
| `node_id` | `NodeId` | The child node's immutable ID |
| `label` | `String` | The child node's label |
| `summary` | `String` | The child node's summary text |
| `gloss` | `String` | The child node's gloss text |

### `PreparedSummary`

A hash-bound transaction handle returned by `prepare_*` and consumed by
`commit_summary`. The `request_id` and `input` fields are public so the
caller can send `input` to a summarizer; the `TransactionToken` is
private and unforgeable — only the namespace can create and verify it.

## High-level async workflows

*Available when the `async-adapters` feature is enabled (default).*

These methods combine prepare, inference, and commit into a single
awaitable call. If inference fails, the pending request is cancelled
and canonical state is not mutated.

### `Summarizer` trait

```rust
#[async_trait]
pub trait Summarizer: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;
    async fn summarize(&self, input: SummaryInput) -> Result<SummaryResult, Self::Error>;
}
```

A harness-supplied abstraction. A Rust agent implements this with its
preferred model SDK. The library calls `summarize` between `prepare_*`
and `commit_summary`, so no mutable state is held during inference.

### `summarize_records_with`

```rust
pub async fn summarize_records_with<S: Summarizer>(
    &mut self,
    selection: RecordSelection,
    label: NodeLabel,
    summarizer: &S,
) -> Result<SummaryReceipt, WorkflowError<S::Error>>
```

Prepares a record-range summary, calls the summarizer, and commits.
On inference failure, cancels the pending request and returns
`WorkflowError::Inference`.

**Errors:** `WorkflowError::Inference` if the summarizer fails;
`WorkflowError::Core` for namespace errors.

### `summarize_text_with`

```rust
pub async fn summarize_text_with<S: Summarizer>(
    &mut self,
    selection: TextSelection,
    label: NodeLabel,
    summarizer: &S,
) -> Result<SummaryReceipt, WorkflowError<S::Error>>
```

Prepares a text-slice summary, calls the summarizer, and commits.
On inference failure, cancels the pending request and returns
`WorkflowError::Inference`.

**Errors:** `WorkflowError::Inference` if the summarizer fails;
`WorkflowError::Core` for namespace errors.

### `maintain_with`

```rust
pub async fn maintain_with<S: Summarizer>(
    &mut self,
    input: MaintenanceInput,
    summarizer: &S,
) -> Result<MaintenanceOutcome, WorkflowError<S::Error>>
```

Plans maintenance, optionally calls the summarizer, and commits if
triggered. If the policy decides no action is needed, returns
`MaintenanceOutcome::NoAction` without calling the summarizer.

**Errors:** `WorkflowError::Inference` if the summarizer fails;
`WorkflowError::Core` for namespace errors.

### `expand_node`

```rust
pub fn expand_node(
    &mut self,
    locator: NodeLocator,
    ttl: NonZeroU32,
) -> Result<ExpansionReceipt, NamespaceError>
```

Convenience wrapper around `expand` that accepts a `NodeLocator` (path
or node ID) and a type-safe non-zero TTL.

**Errors:** `UnknownPath` if the locator cannot be resolved;
`LimitExceeded` if the TTL or active expansion count exceeds limits.

### `MaintenanceOutcome`

| Variant | Fields | Description |
|---|---|---|
| `NoAction` | `stats: StructuralStats` | Policy did not trigger summarization |
| `Summarized` | `SummaryReceipt` | Maintenance summary was committed |

## Expansion

### `expand`

```rust
pub fn expand(&mut self, path: &NamespacePath, ttl_renders: u32) -> Result<ExpansionReceipt, NamespaceError>
```

Activates a summary node for `ttl_renders` successful renders.
Re-expansion sets `ttl = max(current_ttl, requested_ttl)`.

**Errors:** `UnknownPath`, `LimitExceeded` (TTL or active expansion
count), `InvalidSelection` (`ttl_renders` is zero).

### `is_expanded`

```rust
pub fn is_expanded(&self, node_id: NodeId) -> bool
```

Returns whether a node is currently expanded.

### `collapse`

```rust
pub fn collapse(&mut self, node_id: NodeId) -> Result<ExpansionReceipt, NamespaceError>
```

Clears the active expansion on a node, collapsing it immediately. Canonical
state is not modified; only the expansion overlay is cleared.

**Errors:** `UnknownPath` if the node ID does not resolve.

### `ExpansionReceipt`

| Field | Type | Description |
|---|---|---|
| `node_id` | `NodeId` | Expanded node ID |
| `ttl_renders` | `u32` | Effective TTL after max(current, requested) |
| `render_id` | `RenderId` | Render ID associated with the activation |

## Inspection

### `list_nodes`

```rust
pub fn list_nodes(&self) -> Vec<NodeDescriptor>
```

Returns descriptors for all nodes in the tree.

### `inspect_node`

```rust
pub fn inspect_node(&self, locator: NodeLocator) -> Result<NodeDescriptor, NamespaceError>
```

Returns a descriptor for a single node by path or ID. Does not change
TTL state.

**Errors:** `UnknownPath` if the locator does not resolve.

### `resolve_path`

```rust
pub fn resolve_path(&self, path: &NamespacePath) -> Result<NodeId, NamespaceError>
```

Resolves a current path to its node ID.

**Errors:** `UnknownPath`.

### `current_path`

```rust
pub fn current_path(&self, node_id: NodeId) -> Result<NamespacePath, NamespaceError>
```

Returns the current absolute path of a node.

**Errors:** `UnknownPath`.

### `NodeDescriptor`

| Field | Type | Description |
|---|---|---|
| `node_id` | `NodeId` | Immutable node ID |
| `path` | `NamespacePath` | Current absolute path |
| `label` | `String` | Node label |
| `parent_id` | `Option<NodeId>` | Parent node ID, or `None` for root |
| `child_count` | `usize` | Immediate child count |
| `summary_length` | `usize` | Byte length of the summary text (UTF-8) |
| `summary` | `String` | Summary text |
| `gloss` | `String` | Gloss text |
| `expanded` | `bool` | Whether currently expanded |
| `source_kind` | `NodeSourceKind` | `TextSlice` or `RecordRange` |

### `NodeLocator`

| Variant | Fields | Description |
|---|---|---|
| `Path` | `NamespacePath` | Locate by current absolute path |
| `Id` | `NodeId` | Locate by immutable node ID |

## Path types

### `NamespacePath`

A typed segment array representing an absolute path. Paths start with
`/`, use JSON Pointer escaping (`~` → `~0`, `/` → `~1`), and are built
through `parse` or `child`.

| Method | Return type | Description |
|---|---|---|
| `root()` | `Self` | The root path (no segments) |
| `parse(input, limits)` | `Result<Self, PathError>` | Parses an absolute path string |
| `child(&self, label, limits)` | `Result<Self, PathError>` | Appends a label, checking `max_tree_depth` |
| `segments()` | iterator of `&str` | The decoded segments |
| `to_path_string()` | `String` | Renders with JSON Pointer escaping (root renders as `/`) |
| `depth()` | `usize` | Segment count |

### `NodeLabel`

```rust
pub fn new(raw: impl Into<String>, limits: &DefensiveLimits) -> Result<Self, PathError>
```

Creates a normalized label: NFC-normalized, nonempty, free of control
characters, at most `max_label_length` characters.

### `PathError`

| Variant | Description |
|---|---|
| `Empty` | The path input is empty |
| `NotAbsolute` | The path does not start with `/` |
| `EmptySegment` | A path segment or label is empty |
| `DepthExceeded` | The path exceeds `max_tree_depth` |
| `LabelTooLong` | The label exceeds `max_label_length` |
| `ControlChar` | The label contains control characters |

## Validation

### `validate`

```rust
pub fn validate(&self) -> Result<ValidationReport, InvariantViolation>
```

Checks all 12 namespace invariants: forest acyclicity, parent/child
reciprocity, unique ownership, structural ref validity, unique IDs,
path index consistency, alias validation, expansion state validity,
record ID uniqueness, render-field representation, pending state
isolation, and version monotonicity.

**Errors:** `InvariantViolation` if any invariant is violated.

### `ValidationReport`

| Field | Type | Description |
|---|---|---|
| `namespace_version` | `u64` | Namespace version at validation time |
| `checked_records` | `u64` | Canonical records checked |
| `checked_nodes` | `u64` | Summary nodes checked |
| `checked_references` | `u64` | Structural references checked |
| `warnings` | `Vec<ValidationWarning>` | Non-fatal warnings |

### `ValidationWarning`

| Code | Description |
|---|---|
| `DanglingAlias` | An alias points to a missing node |
| `PathMismatch` | An indexed path does not match the derived path |
| `DanglingExpansion` | An expansion references a missing node or has a zero TTL |

### `InvariantViolation` variants

| Variant | Description |
|---|---|
| `Cycle` | A node is its own ancestor |
| `ParentChildMismatch` | Parent/child links are not reciprocal |
| `NoOwner` | A node has no canonical owner |
| `DanglingRef` | A structural reference points to a missing node |
| `DuplicateRecordId` | Two records share the same stable ID |
| `DuplicateNodeId` | Two nodes share the same stable ID |
| `PathIndexMismatch` | A path resolves to the wrong node |
| `UnreachableNode` | A node is unreachable from any root or carrier |
| `AliasShadowsPath` | An alias shadows a current path |
| `DanglingAlias` | An alias points to a missing node |

## Configuration

### `NamespaceConfig`

| Field | Type | Default | Description |
|---|---|---|---|
| `render_field` | `String` | `"content"` | The string field the model sees |
| `carrier` | `CarrierTemplate` | see below | Synthetic carrier record template |
| `render` | `RenderOptions` | see below | Rendered JSON output options |
| `policy` | `PolicyConfig` | see below | Maintenance policy |
| `limits` | `DefensiveLimits` | see below | Defensive ceilings |

`NamespaceConfig::validate` (run by `ContextNamespace::new`) rejects an
empty `render_field`, `trigger_tokens < target_tokens`,
`effective_budget_tokens < trigger_tokens`, `preserve_prefix_records == 0`,
`preserve_recent_records == 0`, and
`default_expansion_ttl > max_expansion_ttl`.

### `PolicyConfig`

| Field | Type | Default | Description |
|---|---|---|---|
| `effective_budget_tokens` | `u32` | 32,000 | Effective token budget for rendered history |
| `trigger_tokens` | `u32` | 24,000 | Maintenance trigger threshold |
| `target_tokens` | `u32` | 16,000 | Target token count for the summarized range |
| `preserve_prefix_records` | `u32` | 1 | Prefix records always preserved |
| `preserve_recent_records` | `u32` | 8 | Most-recent records always preserved |
| `max_summary_chars` | `u32` | 1,200 | Maximum summary characters (effective cap is the tighter of this and the defensive limit) |
| `max_gloss_chars` | `u32` | 160 | Maximum gloss characters (effective cap is the tighter of this and the defensive limit) |
| `default_expansion_ttl` | `NonZeroU32` | 2 | Default expansion TTL in renders |
| `max_expansion_ttl` | `NonZeroU32` | 4 | Maximum expansion TTL in renders |

### `RenderOptions`

| Field | Type | Default | Description |
|---|---|---|---|
| `index_mode` | `IndexMode` | `Both` | Which index metadata is injected |
| `envelope` | `bool` | `true` | Wrap records in a JSON envelope object |
| `pretty` | `bool` | `false` | Pretty-print the rendered JSON |
| `metadata_key` | `String` | `"__ctx"` | Reserved metadata key for index metadata |
| `include_child_glosses` | `bool` | `true` | Include child gloss lists in collapsed summary markers |

### `IndexMode`

| Variant | Behavior |
|---|---|
| `None` | No index metadata injected |
| `RecordId` | Record ID only |
| `VisibleIndex` | Visible index only |
| `Both` | Visible index and record ID (default) |

### `CarrierTemplate`

| Field | Type | Default | Description |
|---|---|---|---|
| `role` | `String` | `"system"` | Role field value for synthetic carrier records |

### `DefensiveLimits`

`DefensiveLimits::portfolio_defaults()` is also the `Default`
implementation. Enforcement status per field is documented in
[How to configure defensive limits](../howtos/configure-limits.md).

| Field | Type | Default |
|---|---|---|
| `max_records` | `NonZeroU32` | 10,000 |
| `max_nodes` | `NonZeroU32` | 10,000 |
| `max_source_bytes` | `NonZeroU64` | 64 MiB |
| `max_render_bytes` | `NonZeroU64` | 16 MiB |
| `max_record_bytes` | `NonZeroU64` | 256 KiB |
| `max_summary_chars` | `NonZeroU32` | 1,200 |
| `max_gloss_chars` | `NonZeroU32` | 160 |
| `max_tree_depth` | `NonZeroU32` | 32 |
| `max_children_per_node` | `NonZeroU32` | 64 |
| `max_active_expansions` | `NonZeroU32` | 64 |
| `max_pending_requests` | `NonZeroU32` | 16 |
| `max_aliases_per_node` | `NonZeroU32` | 16 |
| `retained_render_resolutions` | `NonZeroU32` | 8 |
| `max_label_length` | `NonZeroU32` | 128 |
| `max_event_queue` | `NonZeroU32` | 10,000 |

## Snapshots

### `snapshot`

```rust
pub fn snapshot(&mut self) -> Result<SnapshotBytes, NamespaceError>
```

Serializes the namespace to versioned JSON with a content SHA-256.
Emits a `SnapshotSaved` event with the byte count.

### `SnapshotBytes`

| Method | Return type | Description |
|---|---|---|
| `data()` | `&[u8]` | The serialized JSON bytes |
| `sha256_hex()` | `&str` | Lowercase hex SHA-256 digest of `data()` |

### `restore`

```rust
pub fn restore(bytes: &[u8], options: RestoreOptions) -> Result<(Self, RestoreReceipt), NamespaceError>
```

Rebuilds a namespace from snapshot bytes. Uses `serde_path_to_error`
for field-level decode errors. Validates schema version, rebuilds the
node arena, resolves parent/child links, restores aliases and expansion
state, and runs full invariant validation.

**Errors:** `SnapshotVersion`, `Invariant`.

### `RestoreOptions`

| Field | Type | Default | Description |
|---|---|---|---|
| `min_version` | `Option<u32>` | `None` | Reject snapshots with schema version below this |
| `max_version` | `Option<u32>` | `None` | Reject snapshots with schema version above this |

`RestoreOptions::default()` accepts any schema version. The restore
function always rejects snapshots whose schema version does not match
the compiled `SNAPSHOT_SCHEMA_VERSION`. The `min_version` and
`max_version` fields add optional bounds on top of that check.

## Events

### `drain_events`

```rust
pub fn drain_events(&mut self, limit: usize) -> Vec<NamespaceEvent>
```

Drains up to `limit` events from the bounded internal queue. The queue
drops the oldest event when full and increments a dropped-event
counter.

### `NamespaceEvent` variants

| Variant | Key fields |
|---|---|
| `Append` | `record_id`, `visible_index` |
| `SummaryPrepared` | `request_id` |
| `SummaryCommitted` | `node_id`, `carrier_record_id`, `replaced_record_ids` |
| `SummaryCancelled` | `request_id` |
| `ExpansionActivated` | `node_id`, `ttl_renders` |
| `ExpansionExpired` | `node_id` |
| `Render` | `render_id`, `bytes`, `entries` |
| `SnapshotSaved` | `bytes` |
| `Restored` | `records`, `nodes` |

## Errors

### `NamespaceError`

| Variant | Description |
|---|---|
| `MissingRenderField` | Record lacks the configured string field |
| `StaleRender` | Render ID is stale or unknown |
| `StaleSummary` | Summary request is stale or not found |
| `UnknownPath` | Path or node ID does not resolve |
| `PathCollision` | New node path collides with an existing path |
| `InvalidSelection` | Selection range is invalid |
| `PolicyViolation` | Policy configuration rejects the operation |
| `EmbeddingMismatch` | Embedding dimensions or model ID mismatch |
| `SnapshotVersion` | Snapshot schema version is unsupported |
| `Invariant` | Invariant violation |
| `LimitExceeded` | Defensive limit exceeded |
| `Config` | Configuration error |
| `InvalidSummary` | Summary result fails validation |

### `WorkflowError`

| Variant | Description |
|---|---|
| `Inference` | Wraps an inference error from a `Summarizer` or `Embedder` |
| `Core` | Wraps a `NamespaceError` |

## ID types

| Type | Description |
|---|---|
| `RecordId` | Immutable, opaque identity for a record (UUIDv7) |
| `NodeId` | Immutable, opaque identity for a summary node (UUIDv7) |
| `RenderId` | Immutable identity for one rendered snapshot (UUIDv7) |
| `RequestId` | Immutable identity for a summary request (UUIDv7) |
| `ContentHash` | SHA-256 content hash for source integrity and provenance |

### `ContentHash::from_hex`

```rust
pub fn from_hex(s: &str) -> Result<Self, String>
```

Decodes a 64-character lowercase hex string into a content hash. Returns
an error message if the input is not valid hex or not exactly 32 bytes.

### `Capabilities::detect`

```rust
pub fn detect() -> Capabilities
```

Detects enabled cargo features and schema versions at compile time.
Returns a `Capabilities` struct with `library_version`, `rust_api_version`,
`snapshot_schema`, `render_schema`, `policy_schema`, and `features`.

## Feature flags

| Feature | Default | Adds |
|---|:---:|---|
| `json-schema` | Yes | The `schemars` dependency (no JSON Schema derives are currently generated) |
| `async-adapters` | Yes | `Summarizer` trait, `summarize_records_with`, `summarize_text_with`, `maintain_with`, `expand_node` |
| `semantic-search` | No | Embedding and search data types (full semantic search functionality not yet implemented) |
| `tracing` | No | `tracing` dependency for future instrumentation (instrumentation not yet implemented) |

## High-level wrapper: `SimpleContextNamespace`

A high-level wrapper around `ContextNamespace` that hides transaction
details behind simplified methods. Uses `Arc<Mutex<ContextNamespace>>`
internally so the handle can be cloned for async workflows.

### `SimpleContextNamespace::new`

```rust
pub fn new(config: NamespaceConfig) -> Result<Self, NamespaceError>
```

Creates an empty namespace.

### `SimpleContextNamespace::from_simple_records`

```rust
pub fn from_simple_records(
    config: NamespaceConfig,
    records: &[(&str, &str)],
) -> Result<Self, NamespaceError>
```

Creates a namespace from `(role, content)` tuples.

### `SimpleContextNamespace::append`

```rust
pub fn append(&mut self, role: &str, content: &str) -> Result<AppendReceipt, NamespaceError>
```

Appends a record with the given role and content string.

### `SimpleContextNamespace::render`

```rust
pub fn render(&mut self) -> Result<RenderView, NamespaceError>
```

Renders the namespace as deterministic JSON. `RenderView` is a type
alias for `RenderedHistory`.

### `SimpleContextNamespace::summarize_records`

```rust
pub async fn summarize_records<S: Summarizer>(
    &mut self,
    render_id: &RenderId,
    start: u32,
    end: u32,
    label: &str,
    summarizer: &S,
) -> Result<SummaryReceipt, WorkflowError<S::Error>>
```

Combines prepare, inference, and commit into one awaitable call.
Releases the internal lock during inference, then reacquires it to
commit. On inference failure, cancels the pending request.

### `SimpleContextNamespace::expand`

```rust
pub fn expand(&mut self, path: &str, ttl: u32) -> Result<ExpansionReceipt, NamespaceError>
```

Expands a node by path string.

### `SimpleContextNamespace::collapse`

```rust
pub fn collapse(&mut self, path: &str) -> Result<ExpansionReceipt, NamespaceError>
```

Collapses a node by path string.

### `SimpleContextNamespace::is_expanded`

```rust
pub fn is_expanded(&self, path: &str) -> bool
```

Checks whether a node at the given path is currently expanded.

### `SimpleContextNamespace::list_nodes`

```rust
pub fn list_nodes(&self, filter: NodeFilter) -> Vec<NodeDescriptor>
```

Returns descriptors for nodes matching the given filter.

### `NodeFilter`

| Variant | Description |
|---|---|
| `All` | Return all nodes |
| `ExpandedOnly` | Return only nodes with an active expansion |
| `CollapsedOnly` | Return only nodes without an active expansion |

### `SimpleContextNamespace::validate`

```rust
pub fn validate(&self) -> Result<ValidationReport, InvariantViolation>
```

Validates all namespace invariants.

### `SimpleContextNamespace::snapshot`

```rust
pub fn snapshot(&self) -> Result<SnapshotBytes, NamespaceError>
```

Serializes the namespace to versioned JSON.

### `SimpleContextNamespace::restore`

```rust
pub fn restore(bytes: &[u8], options: RestoreOptions) -> Result<Self, NamespaceError>
```

Rebuilds a namespace from snapshot bytes.

### `SimpleContextNamespace::capabilities`

```rust
pub fn capabilities(&self) -> Capabilities
```

Returns enabled features and schema versions.

### `Capabilities`

| Field | Type | Description |
|---|---|---|
| `library_version` | `String` | Crate version from `Cargo.toml` |
| `rust_api_version` | `String` | High-level Rust API version |
| `snapshot_schema` | `u32` | Snapshot schema version |
| `render_schema` | `u32` | Render schema version |
| `policy_schema` | `u32` | Policy schema version |
| `features` | `HashSet<String>` | Enabled cargo feature names |

### `Capabilities::detect`

```rust
pub fn detect() -> Capabilities
```

Detects enabled cargo features and schema versions at compile time.

## Python bindings

The `context-namespace-py` crate provides a PyO3-based native extension
(`context_namespace._native`) wrapped by a stdlib-only Python facade
package (`context_namespace`).

### Architecture

- **`_native` module** — PyO3 `_NativeNamespace` class with private
  methods. Deliberately awkward to use directly.
- **Python facade** (`context_namespace.ContextNamespace`) — ergonomic
  public API with dataclass DTOs, protocols, and error mapping.
- **No Pydantic** — all DTOs are plain `@dataclass(frozen=True, slots=True)`.
- **Error mapping** — native exceptions are caught and re-raised as
  Python exceptions with the same hierarchy.

### Package layout

```
context_namespace/
  __init__.py    # public re-exports
  config.py      # NamespaceConfig and sub-configs
  errors.py      # Python exception hierarchy
  models.py      # DTOs: receipts, descriptors, stats
  protocols.py   # Summarizer, Embedder protocols
  facade.py      # ContextNamespace class
  _native.pyi    # type stubs for the _native extension
```

### Usage

```python
from context_namespace import ContextNamespace, NamespaceConfig

ns = ContextNamespace(config=NamespaceConfig())
ns.append({"role": "user", "content": "hello"})
view = ns.render()
print(view.json)
```

### Building

```bash
maturin develop
```
