use serde::{Deserialize, Serialize};

use crate::error::NamespaceError;
use crate::id::{ContentHash, NodeId, RecordId, RenderId, RequestId};
use crate::receipt::SummaryReceipt;
use crate::transaction::TransactionToken;

// ---------------------------------------------------------------------------
// Selection types
// ---------------------------------------------------------------------------

/// Locates a single record either by stable ID or by a visible index
/// validated against a specific render.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordLocator {
    /// Locate by immutable record ID.
    Id(RecordId),
    /// Locate by visible index within a specific render snapshot.
    Visible { render_id: RenderId, index: u32 },
}

/// Selects a half-open character range `[start_char, end_char)` within one
/// record's render field for text-slice summarization.
///
/// Character offsets are Unicode scalar-value offsets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextSelection {
    /// How to locate the target record.
    pub record: RecordLocator,
    /// Unicode scalar-value offset (inclusive).
    pub start_char: u32,
    /// Unicode scalar-value offset (exclusive).
    pub end_char: u32,
}

/// Selects a half-open range `[start_index, end_index)` of consecutive
/// canonical records for record-range summarization.
///
/// The `render_id` must come from a fresh render so visible indices can be
/// safely mapped to canonical positions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordSelection {
    /// Fresh render ID for visible-index validation.
    pub render_id: RenderId,
    /// Half-open range start (inclusive).
    pub start_index: u32,
    /// Half-open range end (exclusive).
    pub end_index: u32,
}

// ---------------------------------------------------------------------------
// Summary input / output
// ---------------------------------------------------------------------------

/// A descriptor for an immediate child node of a new summary.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChildDescriptor {
    /// The immutable ID of the child node.
    pub node_id: NodeId,
    /// The child node's label.
    pub label: String,
    /// The child node's summary text.
    pub summary: String,
    /// The child node's gloss text.
    pub gloss: String,
}

/// The input sent to an external summarizer.
///
/// `source` is the rendered text extracted from canonical collapsed state.
/// `children` describes immediate child nodes whose references appear in the
/// source. `max_summary_chars` and `max_gloss_chars` are policy-enforced
/// limits the summarizer must respect.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SummaryInput {
    /// The request ID associated with this summary preparation.
    pub request_id: RequestId,
    /// The rendered source text extracted from canonical collapsed state.
    pub source: String,
    /// Descriptors for immediate child nodes whose references appear in the source.
    pub children: Vec<ChildDescriptor>,
    /// Policy-enforced maximum character length for the summary.
    pub max_summary_chars: u32,
    /// Policy-enforced maximum character length for the gloss.
    pub max_gloss_chars: u32,
}

/// The result returned by an external summarizer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SummaryResult {
    /// The summary text produced by the external summarizer.
    pub summary: String,
    /// The short gloss text for collapsed rendering.
    pub gloss: String,
}

impl SummaryResult {
    /// Validates the result against policy-enforced length limits.
    pub(crate) fn validate(
        &self,
        limits: &crate::config::DefensiveLimits,
    ) -> Result<(), NamespaceError> {
        let max_summary = limits.max_summary_chars.get() as usize;
        let max_gloss = limits.max_gloss_chars.get() as usize;
        if self.summary.chars().count() > max_summary {
            return Err(NamespaceError::InvalidSelection(format!(
                "summary length {} exceeds max {}",
                self.summary.chars().count(),
                max_summary
            )));
        }
        if self.gloss.chars().count() > max_gloss {
            return Err(NamespaceError::InvalidSelection(format!(
                "gloss length {} exceeds max {}",
                self.gloss.chars().count(),
                max_gloss
            )));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Prepared summary (transaction handle)
// ---------------------------------------------------------------------------

/// A prepared, hash-bound summary request awaiting external inference and
/// commit.
///
/// `request_id` and `input` are public so the caller can send `input` to a
/// summarizer. The `token` field is private and unforgeable — only the
/// namespace can create it during `prepare_*` and verify it during
/// `commit_summary`.
#[derive(Debug)]
pub struct PreparedSummary {
    /// The request ID for this prepared summary.
    pub request_id: RequestId,
    /// The input to send to the external summarizer.
    pub input: SummaryInput,
    token: TransactionToken,
}

impl PreparedSummary {
    /// Creates a prepared summary. Only callable from within the crate
    /// (by `ContextNamespace::prepare_*`).
    pub(crate) fn new(request_id: RequestId, input: SummaryInput, token: TransactionToken) -> Self {
        Self {
            request_id,
            input,
            token,
        }
    }

    /// Returns a reference to the transaction token. Used by
    /// `commit_summary` for verification.
    pub(crate) fn token(&self) -> &TransactionToken {
        &self.token
    }
}

// ---------------------------------------------------------------------------
// Maintenance types
// ---------------------------------------------------------------------------

/// A per-record token estimate bound to a stable record ID and the digest of
/// that record's current rendered JSON fragment.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecordTokenEstimate {
    /// The stable record ID this estimate applies to.
    pub record_id: RecordId,
    /// SHA-256 digest of the record's current rendered JSON fragment.
    pub render_digest: ContentHash,
    /// The harness-supplied token estimate for this record.
    pub estimated_tokens: u32,
}

/// Token estimates supplied by the harness before maintenance.
///
/// The whole-render estimate (`estimated_next_tokens`) controls the trigger.
/// Per-record estimates allocate the desired reduction.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MaintenanceInput {
    /// Harness estimate of total tokens in the next render.
    pub estimated_next_tokens: u32,
    /// Per-record token estimates with digests.
    pub records: Vec<RecordTokenEstimate>,
}

/// The decision returned by `plan_maintenance`.
///
/// `NoAction` means the current state is within policy. `Summarize` carries
/// a prepared summary that the harness should send to its summarizer and then
/// commit.
#[derive(Debug)]
pub enum MaintenanceDecision {
    NoAction {
        stats: crate::event::StructuralStats,
    },
    Summarize(PreparedSummary),
}

/// The outcome of a high-level `maintain_with` workflow.
///
/// `NoAction` means the policy did not trigger summarization.
/// `Summarized` carries the commit receipt for the maintenance summary.
#[derive(Clone, Debug)]
pub enum MaintenanceOutcome {
    NoAction {
        stats: crate::event::StructuralStats,
    },
    Summarized(SummaryReceipt),
}
