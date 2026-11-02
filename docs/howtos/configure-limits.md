# How to Configure Defensive Limits

`DefensiveLimits` caps all namespace collections and outputs. These are
defensive ceilings, not benchmark hyperparameters.

## Prerequisites

- A `NamespaceConfig` under construction

## Using portfolio defaults

```rust
use context_namespace::DefensiveLimits;

let limits = DefensiveLimits::portfolio_defaults();
```

These defaults are suitable for agent benchmarking with up to 10,000
records and 10,000 nodes.

## Customizing individual limits

```rust
use std::num::NonZeroU32;
use context_namespace::DefensiveLimits;

let limits = DefensiveLimits {
    max_records: NonZeroU32::new(5_000).unwrap(),
    max_nodes: NonZeroU32::new(2_000).unwrap(),
    max_event_queue: NonZeroU32::new(1_000).unwrap(),
    ..DefensiveLimits::portfolio_defaults()
};
```

## Available limits

| Field | Default | Enforced on |
|---|---|---|
| `max_records` | 10,000 | `append` |
| `max_nodes` | 10,000 | `prepare_*_summary` |
| `max_source_bytes` | 64 MiB | not enforced (reserved) |
| `max_render_bytes` | 16 MiB | `render`, `preview_render` |
| `max_record_bytes` | 256 KiB | `append` |
| `max_summary_chars` | 1,200 | `commit_summary` |
| `max_gloss_chars` | 160 | `commit_summary` |
| `max_tree_depth` | 32 | path construction |
| `max_children_per_node` | 64 | `commit_summary` |
| `max_active_expansions` | 64 | `expand` |
| `max_pending_requests` | 16 | `prepare_*_summary` |
| `max_aliases_per_node` | 16 | not enforced (reserved) |
| `retained_render_resolutions` | 8 | `render` |
| `max_label_length` | 128 | `NodeLabel::new` |
| `max_event_queue` | 10,000 | event push |

Two limits (`max_source_bytes` and `max_aliases_per_node`) are declared and
serialized with the configuration but are not yet enforced by any operation.

`max_summary_chars` and `max_gloss_chars` are upper bounds only: the cap sent
to the summarizer and enforced at commit is the tighter of the defensive limit
and the corresponding `PolicyConfig` value.

## Verification

Limits that are exceeded return `NamespaceError::LimitExceeded`. The
namespace state is not mutated when a limit check fails.
