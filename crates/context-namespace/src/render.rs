use serde::{Deserialize, Serialize};

use crate::id::{ContentHash, NodeId, RecordId, RenderId};

/// The result of a successful [`crate::ContextNamespace::render`] operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenderedHistory {
    /// The unique ID for this render.
    pub render_id: RenderId,
    /// The namespace version at the time of rendering.
    pub namespace_version: u64,
    /// The rendered JSON string.
    pub json: String,
    /// The byte count of `json`.
    pub bytes: u64,
    /// The number of visible entries in the rendered output.
    pub entries: u32,
}

/// A per-record unit for maintenance planning, containing the rendered
/// fragment and its digest.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MaintenanceUnit {
    /// The stable record ID.
    pub record_id: RecordId,
    /// SHA-256 digest of the rendered JSON fragment.
    pub render_digest: ContentHash,
    /// The rendered JSON fragment for this record.
    pub json_fragment: String,
}

/// A preview of a render including maintenance metadata.
///
/// Unlike [`RenderedHistory`], a preview does not allocate a `render_id`,
/// register visible-index resolutions, emit a render event, or decrement
/// expansion TTLs. The harness uses it to estimate per-record token counts
/// before calling `plan_maintenance`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenderPreview {
    /// The namespace version at the time of preview.
    pub namespace_version: u64,
    /// The rendered JSON string.
    pub json: String,
    /// The byte count of `json`.
    pub bytes: u64,
    /// The number of visible entries.
    pub entries: u32,
    /// Per-record maintenance units for token estimation.
    pub maintenance_units: Vec<MaintenanceUnit>,
}

/// A preview of a potential summary operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SummaryPreview {
    /// The label for the proposed summary node.
    pub label: String,
    /// The summary text.
    pub summary: String,
    /// The gloss text.
    pub gloss: String,
    /// The node ID if the summary has been committed, or `None` if it is
    /// only prepared.
    pub node_id: Option<NodeId>,
}
