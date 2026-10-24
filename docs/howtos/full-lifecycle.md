# How to Exercise the Full Namespace Lifecycle

This guide walks through every core operation in a single session: create,
append, render, summarize, expand, inspect, maintain, snapshot, and
restore. Use it as a template for agent integration or as a smoke test
of the library surface.

## Prerequisites

- Rust 1.85+ and the `context-namespace` crate with default features.
- Familiarity with the [getting-started tutorial](../tutorials/getting-started.md).

## Step 1: Create a namespace with a policy

```rust
use context_namespace::{
    ContextNamespace, DefensiveLimits, NamespaceConfig, PolicyConfig,
    RenderOptions, IndexMode,
};

let mut ns = ContextNamespace::new(NamespaceConfig {
    render_field: "content".to_string(),
    policy: PolicyConfig {
        effective_budget_tokens: 32_000,
        trigger_tokens: 24_000,
        target_tokens: 16_000,
        preserve_prefix_records: 1,
        preserve_recent_records: 3,
        max_summary_chars: 1_200,
        max_gloss_chars: 160,
        default_expansion_ttl: 2,
        max_expansion_ttl: 4,
    },
    render: RenderOptions {
        index_mode: IndexMode::Both,
        ..Default::default()
    },
    limits: DefensiveLimits::portfolio_defaults(),
    ..Default::default()
}).expect("valid config");
```

## Step 2: Append records

```rust
use context_namespace::NewRecord;
use indexmap::IndexMap;

for i in 0..10 {
    let mut fields = IndexMap::new();
    fields.insert("role".to_string(), serde_json::json!("user"));
    fields.insert("content".to_string(),
        serde_json::json!(format!("Step {i}: inspect file_{i}.rs and note findings.")));
    ns.append(NewRecord::new(
        fields, "content", &ns.config().limits,
    ).unwrap()).unwrap();
}
```

Each append returns an `AppendReceipt` with a stable `RecordId`.

## Step 3: Render and capture a render ID

```rust
let view = ns.render().expect("render");
assert_eq!(view.entries, 10);
```

The `render_id` is required for any operation that uses visible indices.

## Step 4: Summarize a record range

```rust
use context_namespace::{NodeLabel, RecordSelection, SummaryResult};

let label = NodeLabel::new("batch-1",
    &ns.config().limits).expect("valid label");
let prepared = ns.prepare_record_summary(
    RecordSelection {
        render_id: view.render_id,
        start_index: 0,
        end_index: 5,
    },
    label,
).expect("prepare");

ns.commit_summary(prepared, SummaryResult {
    summary: "Steps 0–4 investigated several files and found edge cases.".to_string(),
    gloss: "Initial investigation".to_string(),
}).expect("commit");
```

Records 0–4 are replaced by one carrier record referencing the node at
`/batch-1`.

## Step 5: Summarize again to create a sibling node

```rust
let view2 = ns.render().expect("render");

let label2 = NodeLabel::new("batch-2",
    &ns.config().limits).expect("valid label");
let prepared2 = ns.prepare_record_summary(
    RecordSelection {
        render_id: view2.render_id,
        start_index: 1,
        end_index: 4,
    },
    label2,
).expect("prepare");

ns.commit_summary(prepared2, SummaryResult {
    summary: "Steps 5–7 refined the parser and added edge-case tests.".to_string(),
    gloss: "Parser refinement".to_string(),
}).expect("commit");
```

The new node at `/batch-2` becomes a sibling of `/batch-1` under the root.
The selected range contains no carrier references, so no reparenting
occurs. (To create a nested node instead, select a range that includes
the carrier record of `/batch-1` — for example `start_index: 0`,
`end_index: 5`. The referenced node is then reparented under the new
summary, and its old path becomes an alias.)

## Step 6: Inspect the tree

```rust
use context_namespace::NodeLocator;

let nodes = ns.list_nodes();
assert_eq!(nodes.len(), 2);

for node in &nodes {
    let desc = ns.inspect_node(
        NodeLocator::Id(node.node_id),
    ).expect("inspect");
    println!("{}: {} (children: {})",
        desc.path, desc.gloss, desc.child_count);
}
```

## Step 7: Expand a node and render with the overlay

```rust
use context_namespace::NamespacePath;

let path = NamespacePath::parse("/batch-1",
    &ns.config().limits).expect("valid path");
ns.expand(&path, 2).expect("expand");

let expanded = ns.render().expect("render");
// The carrier for /batch-1 now inlines the node's stored source text
// instead of its collapsed summary marker.
```

After this render the TTL decrements from 2 to 1. One more render
collapses the node back to its gloss.

## Step 8: Run maintenance planning

```rust
use context_namespace::{MaintenanceInput, RecordTokenEstimate};

let preview = ns.preview_render().expect("preview");

// Build per-record estimates from the preview's maintenance units.
let estimates: Vec<RecordTokenEstimate> = preview.maintenance_units
    .iter()
    .map(|unit| RecordTokenEstimate {
        record_id: unit.record_id,
        render_digest: unit.render_digest.clone(),
        estimated_tokens: 5_000, // from a real tokenizer
    })
    .collect();

let input = MaintenanceInput {
    estimated_next_tokens: 25_000, // above trigger threshold
    records: estimates,
};

let decision = ns.plan_maintenance(input).expect("plan");
```

If `estimated_next_tokens` meets or exceeds `trigger_tokens`, the policy
selects the oldest eligible consecutive range and prepares a summary.
Otherwise it returns `MaintenanceDecision::NoAction`.

## Step 9: Validate invariants

```rust
ns.validate().expect("invariants hold");
```

This checks all 12 namespace invariants: forest acyclicity, unique
ownership, path uniqueness, alias validity, expansion state, and more.

## Step 10: Snapshot and restore

```rust
use context_namespace::RestoreOptions;

let snapshot = ns.snapshot().expect("snapshot");
let (restored, receipt) = ContextNamespace::restore(
    snapshot.data(),
    RestoreOptions::default(),
).expect("restore");

assert_eq!(restored.version(), ns.version());
assert_eq!(restored.stats().nodes, ns.stats().nodes);
```

Snapshots are versioned JSON with a content SHA-256. Pending requests
are excluded.

## Step 11: Drain events

```rust
let events = ns.drain_events(100);
assert!(!events.is_empty());
```

Each event is a typed receipt: `Append`, `SummaryCommitted`,
`ExpansionActivated`, `Render`, `SnapshotSaved`, etc. The queue is
bounded; overflow increments a `dropped_events` counter.

## Checkpoint

At this point you have exercised the full lifecycle:

1. Construction with a configured policy
2. Record append
3. Rendering with render-ID allocation
4. Record-range summarization (twice, creating sibling root nodes)
5. Node inspection
6. Expansion with TTL
7. Maintenance planning with token estimates
8. Invariant validation
9. Snapshot and restore
10. Event drainage

## Common errors

- **`StaleRender`**: You used a `render_id` from before the last
  mutation. Call `render()` again to get a fresh ID.
- **`StaleSummary`**: You tried to commit a prepared summary after
  another mutation changed the namespace version. Prepare again.
- **`UnknownPath`**: The path or node ID does not resolve. Use
  `list_nodes()` to find current paths.
- **`PolicyViolation`**: The maintenance estimates do not match the
  current canonical state. Regenerate estimates from a fresh
  `preview_render()`.

## Next steps

- [API reference](../reference/api.md)
- [Architecture overview](../explanation/architecture.md)
- [How to use async summarizer workflows](async-workflows.md)
