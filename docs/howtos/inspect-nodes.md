# How to Inspect Nodes and Paths

The namespace provides several read-only inspection methods that do not
mutate state or consume expansion TTLs.

## Prerequisites

- A `ContextNamespace` with at least one committed summary

## Listing all nodes

```rust
let nodes = ns.list_nodes();
for desc in &nodes {
    println!("{} -> {} ({} children, expanded: {})",
        desc.path, desc.label, desc.child_count, desc.expanded);
}
```

`list_nodes()` returns a `Vec<NodeDescriptor>` with the node ID, current
path, label, parent ID, child count, summary text, gloss, expanded
status, and source kind.

## Inspecting a single node by path or ID

```rust
use context_namespace::{NodeLocator, NamespacePath};

let path = NamespacePath::parse("/batch-1", &ns.config().limits).unwrap();
let desc = ns.inspect_node(NodeLocator::from_path(path)).expect("found");

// Or by immutable node ID:
let desc = ns.inspect_node(NodeLocator::from_id(node_id)).expect("found");
```

`inspect_node` returns a `NodeDescriptor` including the full summary
text. It does not change TTL state.

## Resolving a path to a node ID

```rust
let path = NamespacePath::parse("/batch-1", &ns.config().limits).unwrap();
let node_id = ns.resolve_path(&path).expect("found");
```

Returns `NamespaceError::UnknownPath` if no live node lives at the path.

## Getting the current path of a node

```rust
let path = ns.current_path(node_id).expect("found");
println!("{}", path.to_path_string());
```

Paths change when a node is reparented during a nested summary. Old
paths become aliases.

## Reading structural statistics

```rust
let stats = ns.stats();
println!("records: {}, nodes: {}, max_depth: {}",
    stats.records, stats.nodes, stats.max_depth);
println!("pending: {}, aliases: {}, expansions: {}, dropped_events: {}",
    stats.pending_requests, stats.alias_count, stats.active_expansions, stats.dropped_events);
```

## Common errors

- **`UnknownPath`**: The path or node ID does not resolve to a live
  node. Both `resolve_path` and `inspect_node` accept current paths
  only, not retained aliases. Use `current_path(node_id)` to find the
  current path of a node after reparenting.
