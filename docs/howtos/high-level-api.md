# How to Use the High-Level Rust API

The `SimpleContextNamespace` wrapper provides a small set of
easy-to-understand methods that hide the low-level transaction details.
Use it when you want append, render, summarize, expand, and snapshot
without manually managing `IndexMap` construction, `NodeLabel` validation,
or two-phase prepare/commit transactions.

## Prerequisites

- Rust 1.85+ and the `context-namespace` crate with default features.
- For async summarize workflows, a type implementing `Summarizer`.

## When to use `SimpleContextNamespace` vs. `ContextNamespace`

Use `SimpleContextNamespace` when you want:

- Append by `(role, content)` instead of building an `IndexMap`
- One-call summarize that handles prepare, inference, and commit
- Expand and collapse by path string
- Filtered node listing with `NodeFilter`
- Capabilities reporting

Use `ContextNamespace` directly when you need:

- Manual two-phase transactions (prepare, then commit later)
- Text-slice summarization
- Maintenance planning with real token estimates
- Preview renders without TTL decrement
- Direct access to the event queue

## Creating a namespace

```rust
use context_namespace::{NamespaceConfig, SimpleContextNamespace};

let mut ns = SimpleContextNamespace::new(NamespaceConfig {
    render_field: "content".to_string(),
    ..Default::default()
}).expect("valid config");
```

You can also construct from an initial set of records:

```rust
let ns = SimpleContextNamespace::from_simple_records(
    NamespaceConfig {
        render_field: "content".to_string(),
        ..Default::default()
    },
    &[("user", "hello"), ("assistant", "hi there")],
).expect("construct");
```

Each tuple is `(role, content)`. The content string goes into the render
field specified by the config.

## Appending records

```rust
let receipt = ns.append("user", "Step 1: inspect the file.").expect("append");
println!("Record {} at index {}", receipt.record_id, receipt.visible_index);
```

The receipt contains a stable `RecordId` and the zero-based visible index.

## Rendering

```rust
let view = ns.render().expect("render");
println!("{} entries, {} bytes", view.entries, view.bytes);

// Store the render_id for summarize operations.
let render_id = view.render_id;
```

The `render_id` is required for `summarize_records`. It becomes stale
after any canonical mutation (append or commit). If you get a `StaleRender`
error, call `render()` again for a fresh ID.

The example below summarizes visible indices 0..3, so append at least
four records and capture a fresh `render_id` as shown above before running
it. The full lifecycle example at the end shows a complete runnable
sequence.

## Summarizing records

The `summarize_records` method combines prepare, inference, and commit
into a single awaitable call. You supply a `Summarizer` implementation
that calls your model:

```rust
use async_trait::async_trait;
use context_namespace::{Summarizer, SummaryInput, SummaryResult};

struct MySummarizer;

#[async_trait]
impl Summarizer for MySummarizer {
    type Error = std::convert::Infallible;

    async fn summarize(&self, input: SummaryInput) -> Result<SummaryResult, Self::Error> {
        Ok(SummaryResult {
            summary: format!("Summary of {} chars", input.source.len()),
            gloss: "brief".to_string(),
        })
    }
}

let summarizer = MySummarizer;
let receipt = ns
    .summarize_records(&render_id, 0, 3, "batch-1", &summarizer)
    .await
    .expect("summarize");
```

If the summarizer fails, the pending request is cancelled and canonical
state is not mutated. The error is returned as `WorkflowError::Inference`.

## Expanding and collapsing nodes

```rust
// Expand /batch-1 for 2 renders.
let receipt = ns.expand("/batch-1", 2).expect("expand");
assert!(ns.is_expanded("/batch-1"));

// Render with the expansion active — the node's stored source text
// is inlined instead of its collapsed marker.
let _view = ns.render().expect("render");

// Collapse immediately (optional — TTL auto-collapses at zero).
ns.collapse("/batch-1").expect("collapse");
assert!(!ns.is_expanded("/batch-1"));
```

## Listing nodes

```rust
use context_namespace::NodeFilter;

// All nodes.
let all = ns.list_nodes(NodeFilter::All);

// Only expanded nodes.
let expanded = ns.list_nodes(NodeFilter::ExpandedOnly);

// Only collapsed nodes.
let collapsed = ns.list_nodes(NodeFilter::CollapsedOnly);

for node in &all {
    println!("{}: {} (children: {})", node.path, node.gloss, node.child_count);
}
```

## Checking capabilities

```rust
let caps = ns.capabilities();
println!("Library version: {}", caps.library_version);
println!("Snapshot schema: {}", caps.snapshot_schema);
println!("Features: {:?}", caps.features);
```

The `features` set reports which cargo features are enabled
(`json-schema`, `async-adapters`, `semantic-search`, `tracing`).

## Validating invariants

```rust
ns.validate().expect("invariants hold");
```

This checks all 12 namespace invariants: forest acyclicity, unique
ownership, path uniqueness, alias validity, expansion state, and more.

## Snapshot and restore

```rust
use context_namespace::RestoreOptions;

let snapshot = ns.snapshot().expect("snapshot");
let restored = SimpleContextNamespace::restore(
    snapshot.data(),
    RestoreOptions::default(),
).expect("restore");

assert_eq!(restored.version(), ns.version());
assert_eq!(restored.stats().nodes, ns.stats().nodes);
```

## Full lifecycle example

```rust,no_run
# async fn run() -> Result<(), Box<dyn std::error::Error>> {
use context_namespace::{
    NamespaceConfig, NodeFilter, RestoreOptions, SimpleContextNamespace,
    Summarizer, SummaryInput, SummaryResult,
};
use async_trait::async_trait;

struct MySummarizer;

#[async_trait]
impl Summarizer for MySummarizer {
    type Error = std::convert::Infallible;
    async fn summarize(&self, input: SummaryInput) -> Result<SummaryResult, Self::Error> {
        Ok(SummaryResult {
            summary: format!("Summarized {} chars", input.source.len()),
            gloss: "brief".to_string(),
        })
    }
}

let mut ns = SimpleContextNamespace::new(NamespaceConfig {
    render_field: "content".to_string(),
    ..Default::default()
})?;

// Append records.
for i in 0..8 {
    ns.append("user", &format!("Step {i}: do thing {i}."))?;
}

// Render to get a render ID.
let view = ns.render()?;

// Summarize records 0–4.
let summarizer = MySummarizer;
ns.summarize_records(&view.render_id, 0, 5, "batch-1", &summarizer).await?;

// Render again for a fresh ID.
let view2 = ns.render()?;

// Summarize more records to create a sibling.
ns.summarize_records(&view2.render_id, 1, 3, "batch-2", &summarizer).await?;

// Expand /batch-1.
ns.expand("/batch-1", 2)?;
let _expanded_view = ns.render()?;

// Collapse /batch-1.
ns.collapse("/batch-1")?;

// List nodes.
let nodes = ns.list_nodes(NodeFilter::All);
assert_eq!(nodes.len(), 2);

// Validate.
ns.validate()?;

// Snapshot and restore.
let snapshot = ns.snapshot()?;
let restored = SimpleContextNamespace::restore(snapshot.data(), RestoreOptions::default())?;
assert_eq!(restored.version(), ns.version());

// Restored namespace validates too.
restored.validate()?;
# Ok(())
# }
```

## Error handling

- **`StaleRender`**: The `render_id` was invalidated by a mutation. Call
  `render()` again.
- **`StaleSummary`**: You tried to commit a prepared summary after
  another mutation. Re-prepare.
- **`UnknownPath`**: The path does not resolve. Use `list_nodes()` to
  find current paths.
- **`WorkflowError::Inference(e)`**: The summarizer failed. State is
  clean; retry with a fresh render.
- **`WorkflowError::Core(e)`**: A namespace operation failed after
  inference. Do not retry blindly on `StaleSummary`.

## Next steps

- [Full namespace lifecycle with the low-level API](full-lifecycle.md)
- [API reference](../reference/api.md)
- [Architecture overview](../explanation/architecture.md)
