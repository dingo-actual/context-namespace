use serde::{Deserialize, Serialize};

use crate::id::{NodeId, RecordId, RenderId, RequestId};

/// Snapshot of structural counts at a point in time.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct StructuralStats {
    /// Number of canonical records in the sequence.
    pub records: u32,
    /// Number of summary nodes in the tree.
    pub nodes: u32,
    /// Number of currently active expansions (TTL > 0).
    pub active_expansions: u32,
    /// Number of pending (uncommitted) summary requests.
    pub pending_requests: u32,
    /// Number of retained path aliases.
    pub alias_count: u32,
    /// Maximum depth of the summary tree (0 = no nodes, 1 = root only).
    pub max_depth: u32,
    /// Number of events dropped due to event queue overflow.
    pub dropped_events: u32,
}

/// Events emitted by [`crate::ContextNamespace`] for observability and
/// tracing.
///
/// Events are queued internally and drained via
/// [`crate::ContextNamespace::drain_events`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NamespaceEvent {
    /// A record was appended to the canonical sequence.
    Append {
        record_id: RecordId,
        visible_index: u32,
    },
    /// A summary request was prepared and is pending commit or cancel.
    SummaryPrepared { request_id: RequestId },
    /// A summary was committed: a new node was created and records were
    /// replaced.
    SummaryCommitted {
        node_id: NodeId,
        carrier_record_id: RecordId,
        replaced_record_ids: Vec<RecordId>,
    },
    /// A pending summary request was cancelled.
    SummaryCancelled { request_id: String },
    /// A node expansion was activated with the given TTL.
    ExpansionActivated { node_id: NodeId, ttl_renders: u32 },
    /// A node expansion expired after its TTL reached zero.
    ExpansionExpired { node_id: NodeId },
    /// A render was produced.
    Render {
        render_id: RenderId,
        bytes: u64,
        entries: u32,
    },
    /// A snapshot was saved.
    SnapshotSaved { bytes: u64 },
    /// A snapshot was restored.
    Restored { records: u32, nodes: u32 },
}
