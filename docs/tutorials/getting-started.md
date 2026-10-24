# Getting Started with context-namespace

In this tutorial, we will build a small agent-history namespace from
scratch. We will append records, summarize a range of records into a
node, expand that node for inspection, render the result as JSON, and
validate all invariants. You can complete it in under 30 minutes.

You will need Rust 1.85+ and the `context-namespace` crate.

## Step 1: Create a namespace

Create a new Cargo project, then add the crate to its `Cargo.toml`:

```sh
cargo new context-demo
cd context-demo
```

```toml
[dependencies]
context-namespace = "0.1"
indexmap = "2"
```

Create an empty namespace with default configuration:

```rust
use context_namespace::{ContextNamespace, NamespaceConfig};

let mut ns = ContextNamespace::new(NamespaceConfig {
    render_field: "content".to_string(),
    ..Default::default()
}).expect("valid config");
```

The `render_field` names the string field that the model sees. It
defaults to `"content"`.

## Step 2: Append records

Each record is an `IndexMap<String, serde_json::Value>` with a string
value in the render field:

```rust
use context_namespace::{NewRecord, DefensiveLimits};
use indexmap::IndexMap;

for i in 0..5 {
    let mut fields = IndexMap::new();
    fields.insert("role".to_string(), serde_json::json!("user"));
    fields.insert("content".to_string(), serde_json::json!(format!("record {i}")));
    ns.append(
        NewRecord::new(fields, "content", &DefensiveLimits::portfolio_defaults()).unwrap()
    ).unwrap();
}
```

Each `append` returns an `AppendReceipt` with a stable `RecordId` and a
zero-based `visible_index`.

## Step 3: Render the current state

Rendering produces deterministic JSON and a fresh `RenderId`:

```rust
let view = ns.render().expect("render");
assert_eq!(view.entries, 5);
```

The `RenderId` is required for any operation that uses visible indices,
such as record-range summarization. It becomes stale after any canonical
mutation (an append or a summary commit). Expansion, collapse, and
rendering do not invalidate it.

## Step 4: Summarize a range of records

Summarization uses a two-phase transaction: prepare, then commit.

```rust
use context_namespace::{NodeLabel, RecordSelection, SummaryResult};

let label = NodeLabel::new("batch-1", &ns.config().limits).expect("valid label");
let prepared = ns.prepare_record_summary(
    RecordSelection {
        render_id: view.render_id,
        start_index: 0,
        end_index: 3,
    },
    label,
).expect("prepare");

ns.commit_summary(prepared, SummaryResult {
    summary: "Records 0 through 2".to_string(),
    gloss: "First batch".to_string(),
}).expect("commit");
```

After commit, records 0-2 are replaced by one synthetic carrier record
that references the new summary node at path `/batch-1`.

## Step 5: Expand the summary node

Expansion is a read-only overlay. Canonical state does not change.

```rust
use context_namespace::NamespacePath;

let path = NamespacePath::parse("/batch-1", &ns.config().limits).expect("valid path");
ns.expand(&path, 2).expect("expand");
```

The node is now visible for the next 2 successful renders.

## Step 6: Render with the expansion active

```rust
let expanded_view = ns.render().expect("render");
// The carrier record now inlines the node's stored source — the
// original texts of records 0 through 2, joined by newlines — instead
// of its collapsed marker
// [summary(/first-summary): Records 0 through 2].
```

After this render, the TTL decrements from 2 to 1. One more render
collapses the node back to its gloss.

## Step 7: Validate invariants

```rust
ns.validate().expect("invariants hold");
```

This checks all 12 namespace invariants: forest acyclicity, unique
ownership, path uniqueness, alias validity, expansion state, and more.

## Step 8: Save and restore a snapshot

```rust
use context_namespace::RestoreOptions;

let snapshot = ns.snapshot().expect("snapshot");
let (restored, receipt) = ContextNamespace::restore(
    snapshot.data(),
    RestoreOptions::default(),
).expect("restore");

assert_eq!(restored.version(), ns.version());
```

Snapshots are versioned JSON with a content SHA-256. Pending requests
are excluded.

## What you learned

You now know how to:

- construct a namespace and append records
- render the current state as deterministic JSON
- summarize a record range with a two-phase transaction
- expand a summary node with a TTL
- validate all invariants
- snapshot and restore namespace state

## Next steps

- [How to summarize a text slice within one record](../howtos/summarize-text-slice.md)
- [How to inspect nodes and paths](../howtos/inspect-nodes.md)
- [How to configure defensive limits](../howtos/configure-limits.md)
- [Architecture overview](../explanation/architecture.md)
