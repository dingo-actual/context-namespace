# How to Snapshot and Restore Namespace State

Snapshots serialize the complete canonical state to versioned JSON with
a content hash. Restore rebuilds the namespace with full invariant
validation.

## Prerequisites

- A `ContextNamespace` with records and/or summary nodes

## Saving a snapshot

```rust
let snapshot = ns.snapshot().expect("snapshot");
```

`SnapshotBytes` holds the JSON bytes and a SHA-256 hex digest. You can
write `snapshot.data()` to a file or send it over a network.

## Restoring from a snapshot

```rust
use context_namespace::RestoreOptions;

let (restored, receipt) = ContextNamespace::restore(
    snapshot.data(),
    RestoreOptions::default(),
).expect("restore");
```

`RestoreOptions` controls which snapshot schema versions are accepted:

- `min_version`: reject snapshots whose schema version is below this
  bound (`None` by default — no lower bound)
- `max_version`: reject snapshots whose schema version is above this
  bound (`None` by default — no upper bound)

`RestoreOptions::default()` accepts any schema version. Restore always
rejects a snapshot whose schema version does not match the compiled
`SNAPSHOT_SCHEMA_VERSION`; the two bounds add optional extra checks on
top of that.

## What is excluded from snapshots

Pending (uncommitted) summary requests are not serialized. A restored
namespace starts with an empty pending map. This prevents restoring
committable transactions that may be stale.

## What is included

- Schema version and library version
- Namespace configuration
- Canonical records with stable record IDs
- All nodes, sources, paths, aliases, and provenance
- Active expansion TTLs
- Namespace version
- Content SHA-256

## Verification

After restore, the namespace version and all node paths match the
original. `ns.validate()` passes on the restored instance.
