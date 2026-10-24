use sha2::{Digest, Sha256};

use crate::id::{ContentHash, RecordId, RequestId};

/// An opaque, hash-bound transaction token that authorizes a summary commit.
///
/// The token is constructed only by `prepare_record_summary` or
/// `prepare_text_summary` and verified by `commit_summary`. It binds the
/// commit to a specific request ID, source hash, and namespace version,
/// preventing stale or forged commits.
///
/// The field is private and the struct is not serializable.
#[derive(Debug)]
pub struct TransactionToken {
    request_id: RequestId,
    source_hash: ContentHash,
    namespace_version: u64,
    /// A nonce over all binding fields to make the token unforgeable.
    seal: [u8; 32],
}

impl TransactionToken {
    /// Creates a transaction token binding the commit to the given
    /// request, source hash, and namespace version.
    pub(crate) fn new(
        request_id: RequestId,
        source_hash: ContentHash,
        namespace_version: u64,
    ) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(request_id.as_uuid().as_bytes());
        hasher.update(source_hash.as_bytes());
        hasher.update(namespace_version.to_le_bytes());
        let seal = hasher.finalize().into();
        Self {
            request_id,
            source_hash,
            namespace_version,
            seal,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn request_id(&self) -> RequestId {
        self.request_id
    }

    #[allow(dead_code)]
    pub(crate) fn source_hash(&self) -> &ContentHash {
        &self.source_hash
    }

    #[allow(dead_code)]
    pub(crate) fn namespace_version(&self) -> u64 {
        self.namespace_version
    }

    /// Verifies that this token matches the given request ID, source hash,
    /// and namespace version. The seal is recomputed and compared.
    pub(crate) fn verify(
        &self,
        request_id: RequestId,
        source_hash: &ContentHash,
        namespace_version: u64,
    ) -> bool {
        self.request_id == request_id
            && &self.source_hash == source_hash
            && self.namespace_version == namespace_version
            && {
                let mut hasher = Sha256::new();
                hasher.update(request_id.as_uuid().as_bytes());
                hasher.update(source_hash.as_bytes());
                hasher.update(namespace_version.to_le_bytes());
                let expected: [u8; 32] = hasher.finalize().into();
                expected == self.seal
            }
    }
}

// ---------------------------------------------------------------------------
// Pending summary (internal state)
// ---------------------------------------------------------------------------

/// Internal pending state for a prepared summary request.
///
/// Stored in [`crate::ContextNamespace`] pending map until commit or cancel.
/// Any canonical mutation invalidates pending requests by version mismatch.
#[derive(Debug)]
pub struct PendingSummary {
    /// The request ID for this pending summary.
    pub request_id: crate::id::RequestId,
    /// Namespace version at the time of preparation.
    pub created_version: u64,
    /// SHA-256 hash of the source text.
    pub source_hash: ContentHash,
    /// The canonical record indices being summarized.
    pub record_indices: Vec<usize>,
    /// IDs of records being replaced.
    pub source_record_ids: Vec<crate::id::RecordId>,
    /// IDs of child nodes that will be reparented.
    pub child_node_ids: Vec<crate::id::NodeId>,
    /// The label for the new node.
    pub label: crate::path::NodeLabel,
    /// The operation kind (text-slice or record-range).
    pub operation: PreparedOperation,
}

/// Describes what kind of summary was prepared.
#[derive(Clone, Debug)]
pub enum PreparedOperation {
    /// Summarizing a consecutive range of records.
    RecordRange,
    /// Summarizing a character range within one record's render field.
    TextSlice {
        record_id: crate::id::RecordId,
        start_char: u32,
        end_char: u32,
    },
}

// ---------------------------------------------------------------------------
// Mutation plan (built off-state before commit)
// ---------------------------------------------------------------------------

/// A fully validated mutation plan ready to apply atomically.
///
/// All fallible checks (path collisions, cycle constraints, size limits)
/// are performed before constructing the plan, so `apply` is infallible.
#[derive(Debug)]
pub struct SummaryMutationPlan {
    /// The immutable ID for the new node.
    pub new_node_id: crate::id::NodeId,
    /// The absolute path for the new node.
    pub new_path: crate::path::NamespacePath,
    /// The label for the new node.
    pub label: crate::path::NodeLabel,
    /// The summary text.
    pub summary: String,
    /// The gloss text.
    pub gloss: String,
    /// The compressed source material.
    pub source: crate::node::NodeSource,
    /// Provenance metadata.
    pub provenance: crate::node::Provenance,
    /// Where the node's canonical reference lives.
    pub owner: crate::node::OwnerLocation,
    /// Records to remove from the canonical sequence (record-range only).
    pub remove_record_range: Option<(usize, usize)>,
    /// Carrier record to insert (record-range only).
    pub carrier_record: Option<crate::record::Record>,
    /// Carrier insertion position.
    pub carrier_position: Option<usize>,
    /// Text-slice edit: which record, which segment range to replace.
    pub text_edit: Option<TextEdit>,
    /// Nodes to reparent under the new node.
    pub reparent: Vec<crate::node::NodeKey>,
    /// Old paths to add as aliases.
    pub alias_additions: Vec<(crate::path::NamespacePath, crate::id::NodeId)>,
}

/// Describes a text-slice replacement within one record's render segments.
#[derive(Clone, Debug)]
pub struct TextEdit {
    /// The canonical index of the record being edited.
    pub record_index: usize,
    /// Segment index where the replacement starts.
    pub start_segment: usize,
    /// Number of segments to replace (may be 0 for insert).
    pub replace_count: usize,
    /// New segments to insert at the replacement point.
    pub new_segments: Vec<crate::record::Segment>,
    /// Remaining text from the original that was not summarized.
    pub prefix_text: String,
    /// Remaining text after the summarized range.
    pub suffix_text: String,
}

impl SummaryMutationPlan {
    /// Returns the record IDs being replaced by this plan.
    pub fn source_record_ids(&self) -> Vec<RecordId> {
        match &self.source {
            crate::node::NodeSource::RecordRange { records } => {
                records.iter().map(|r| r.id()).collect()
            }
            crate::node::NodeSource::TextSlice { record_id, .. } => vec![*record_id],
        }
    }
}
