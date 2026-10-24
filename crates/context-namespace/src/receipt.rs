use serde::{Deserialize, Serialize};

use crate::id::{NodeId, RecordId, RenderId};

/// Confirmation of a successful [`crate::ContextNamespace::append`] operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppendReceipt {
    /// The stable ID assigned to the inserted record.
    pub record_id: RecordId,
    /// The zero-based visible index of the record in the canonical sequence.
    pub visible_index: u32,
}

/// Confirmation of a successful [`crate::ContextNamespace::commit_summary`]
/// operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SummaryReceipt {
    /// The ID of the newly created summary node.
    pub new_node_id: NodeId,
    /// The ID of the synthetic carrier record that replaces the summarized
    /// range (record-range only; for text-slice this is the edited record's
    /// ID).
    pub carrier_record_id: RecordId,
    /// IDs of the canonical records replaced by the summary.
    pub replaced_record_ids: Vec<RecordId>,
    /// The render ID associated with the commit.
    pub render_id: RenderId,
}

/// Confirmation of a successful
/// [`crate::ContextNamespace::cancel_summary`] operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CancelReceipt {
    /// The string representation of the cancelled request ID.
    pub request_id: String,
}

/// Confirmation of a successful [`crate::ContextNamespace::expand`]
/// operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExpansionReceipt {
    /// The ID of the expanded node.
    pub node_id: NodeId,
    /// The effective TTL in renders after applying max(current, requested).
    pub ttl_renders: u32,
    /// The render ID associated with the activation.
    pub render_id: RenderId,
}

/// Confirmation of a successful render preview operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenderReceipt {
    /// The unique ID for this render.
    pub render_id: RenderId,
    /// The total byte count of the rendered JSON.
    pub bytes: u64,
    /// The number of visible entries in the rendered output.
    pub entries: u32,
}

/// Confirmation of a successful snapshot restore operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RestoreReceipt {
    /// The number of records restored.
    pub records: u32,
    /// The number of nodes restored.
    pub nodes: u32,
}
