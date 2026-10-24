# How to Use Async Summarizer Workflows

The `async-adapters` feature (enabled by default) provides high-level
async workflows that combine prepare, inference, and commit into a
single awaitable call.

## Prerequisites

- The `async-adapters` feature is enabled (default).
- A type implementing `Summarizer` that calls your model SDK.

## Implementing the Summarizer trait

```rust
use async_trait::async_trait;
use context_namespace::{Summarizer, SummaryInput, SummaryResult};

struct MySummarizer { /* model client */ }

#[async_trait]
impl Summarizer for MySummarizer {
    type Error = MyError;

    async fn summarize(&self, input: SummaryInput) -> Result<SummaryResult, Self::Error> {
        // Send input.source to your model, parse the response.
        Ok(SummaryResult {
            summary: "model output".to_string(),
            gloss: "short gloss".to_string(),
        })
    }
}
```

## Summarizing a record range

```rust
use context_namespace::{NodeLabel, RecordLocator, RecordSelection, TextSelection};

let view = ns.render()?;
let label = NodeLabel::new("batch1", &ns.config().limits)?;

let receipt = ns.summarize_records_with(
    RecordSelection {
        render_id: view.render_id,
        start_index: 0,
        end_index: 3,
    },
    label,
    &summarizer,
).await?;
```

If inference fails, the pending request is cancelled and
`WorkflowError::Inference` is returned. Canonical state is not mutated.

## Summarizing a text slice

```rust
let label = NodeLabel::new("slice1", &ns.config().limits)?;
let receipt = ns.summarize_text_with(
    TextSelection {
        record: RecordLocator::Id(record_id),
        start_char: 0,
        end_char: 100,
    },
    label,
    &summarizer,
).await?;
```

## Running automatic maintenance

```rust
let preview = ns.preview_render()?;
let estimate = my_estimator.estimate(&preview);

let outcome = ns.maintain_with(estimate, &summarizer).await?;

match outcome {
    MaintenanceOutcome::NoAction { .. } => { /* below trigger */ }
    MaintenanceOutcome::Summarized(receipt) => { /* committed */ }
}
```

When the policy decides no action is needed, the summarizer is not
called.

## Expanding by path or ID

```rust
use std::num::NonZeroU32;

let receipt = ns.expand_node(
    NodeLocator::from_id(node_id),
    NonZeroU32::new(2)?,
)?;
```

## Error handling

All async workflows return `WorkflowError<S::Error>`:

- `WorkflowError::Inference(e)` — the summarizer failed; state is clean.
- `WorkflowError::Core(e)` — a namespace operation failed after inference.

Never retry on `WorkflowError::Core(StaleSummary)` — the source
selection may have changed.
