//! Hierarchical, path-addressable context compaction for LLM agents.
//!
//! This crate provides a deterministic data structure for managing ordered
//! agent-history records with nested, path-addressable summaries. An agent
//! can compress old history into summary nodes, expand them temporarily for
//! inspection, and render deterministic JSON for model consumption.
//!
//! # Core concepts
//!
//! - [`ContextNamespace`] — the central state machine owning records, the
//!   summary tree, policy enforcement, and rendering.
//! - [`SimpleContextNamespace`] — a high-level wrapper that hides
//!   transaction details behind simplified methods (append by role/content,
//!   one-call summarize, expand/collapse by path string).
//! - [`NamespacePath`] — human-readable absolute path to a summary node.
//! - [`RecordId`] / [`NodeId`] — immutable opaque identities for records and
//!   nodes.
//! - [`Segment`] — one part of a render field: literal text or a structural
//!   reference to a summary node.
//! - [`PreparedSummary`] — a hash-bound transaction handle returned by
//!   `prepare_*` and consumed by [`ContextNamespace::commit_summary`].
//!
//! # Capabilities
//!
//! The crate implements the following operations:
//!
//! - **Construction**: [`ContextNamespace::new`] and
//!   [`ContextNamespace::from_records`] create a namespace from a
//!   configuration and optional initial records.
//! - **Append**: [`ContextNamespace::append`] adds records with stable
//!   immutable IDs.
//! - **Record-range summarization**: [`ContextNamespace::prepare_record_summary`]
//!   validates a consecutive record range, extracts source text, and returns
//!   a prepared transaction. [`ContextNamespace::commit_summary`] applies it
//!   atomically, replacing the range with a synthetic carrier record.
//! - **Text-slice summarization**: [`ContextNamespace::prepare_text_summary`]
//!   validates a Unicode scalar-value character range within one record's
//!   render field and commits via the same `commit_summary` path.
//! - **Expansion**: [`ContextNamespace::expand`] activates a summary node
//!   for *N* successful renders. TTLs decrement after each successful
//!   render and auto-collapse at zero.
//! - **Rendering**: [`ContextNamespace::render`] produces deterministic JSON
//!   with optional envelope, visible indices, stable record IDs, and
//!   expansion overlay.
//! - **Maintenance planning**: [`ContextNamespace::preview_render`] produces
//!   a non-mutating render preview with per-record maintenance units for
//!   token estimation. [`ContextNamespace::plan_maintenance`] validates
//!   harness-supplied estimates, checks the trigger threshold, and prepares
//!   a record-range summary for the oldest eligible consecutive records.
//! - **Inspection**: [`ContextNamespace::list_nodes`],
//!   [`ContextNamespace::resolve_path`], [`ContextNamespace::current_path`],
//!   and [`ContextNamespace::stats`] provide read-only structural views.
//! - **Validation**: [`ContextNamespace::validate`] checks all 12 namespace
//!   invariants and returns a [`ValidationReport`].
//! - **Snapshots**: [`ContextNamespace::snapshot`] serializes state to JSON
//!   with a content hash. [`ContextNamespace::restore`] rebuilds a namespace
//!   from snapshot bytes with full invariant validation.
//! - **Events**: [`ContextNamespace::drain_events`] returns typed operation
//!   receipts from a bounded internal queue.
//! - **High-level workflows**: when the `async-adapters` feature is enabled,
//!   [`Summarizer`] trait implementations can be used with
//!   [`ContextNamespace::summarize_records_with`],
//!   [`ContextNamespace::summarize_text_with`], and
//!   [`ContextNamespace::maintain_with`] to combine prepare, inference, and
//!   commit into a single awaitable call.
//!
//! # Transactional model
//!
//! Summary commits use a two-phase transaction:
//!
//! 1. `prepare_*` validates the selection and returns a [`PreparedSummary`]
//!    with an unforgeable [`TransactionToken`](crate::transaction::TransactionToken).
//! 2. `commit_summary` revalidates version, source hash, and token, then
//!    applies the mutation atomically. A stale request returns
//!    [`NamespaceError::StaleSummary`] without mutating state.
//!
//! `cancel_summary` removes a pending request without mutating canonical state.
//!
//! # Features
//!
//! | Feature | Default | Adds |
//! |---|:---:|---|
//! | `json-schema` | Yes | The `schemars` dependency (no JSON Schema derives are currently generated) |
//! | `async-adapters` | Yes | High-level `Summarizer` trait and async workflow helpers |
//! | `semantic-search` | No | Embedding and search data types (full semantic search functionality not yet implemented) |
//! | `tracing` | No | `tracing` integration support (instrumentation not yet implemented) |
//!
//! # Example
//!
//! This example exercises the full lifecycle: create, append, render,
//! summarize, expand, snapshot, and restore.
//!
//! ```
//! use context_namespace::{
//!     ContextNamespace, NamespaceConfig, NamespacePath, NewRecord, NodeLabel,
//!     RecordSelection, RestoreOptions, SummaryResult,
//! };
//! use indexmap::IndexMap;
//!
//! // Create a namespace with default configuration.
//! let mut ns = ContextNamespace::new(NamespaceConfig {
//!     render_field: "content".to_string(),
//!     ..Default::default()
//! }).expect("valid config");
//!
//! // Append records.
//! for i in 0..5 {
//!     let mut fields = IndexMap::new();
//!     fields.insert("content".to_string(), serde_json::json!(format!("record {i}")));
//!     fields.insert("role".to_string(), serde_json::json!("user"));
//!     ns.append(NewRecord::new(fields, "content",
//!         &context_namespace::DefensiveLimits::default()).unwrap()).unwrap();
//! }
//!
//! // Render to get a fresh render ID.
//! let view = ns.render().expect("render");
//!
//! // Summarize records 0–2.
//! let label = NodeLabel::new("first-summary",
//!     &ns.config().limits).expect("valid label");
//! let prepared = ns.prepare_record_summary(RecordSelection {
//!     render_id: view.render_id,
//!     start_index: 0,
//!     end_index: 3,
//! }, label).expect("prepare");
//!
//! let result = SummaryResult {
//!     summary: "Summary of records 0–2".to_string(),
//!     gloss: "First three records".to_string(),
//! };
//! ns.commit_summary(prepared, result).expect("commit");
//!
//! // Expand the summary node for 2 renders.
//! let path = NamespacePath::parse("/first-summary",
//!     &ns.config().limits).expect("valid path");
//! ns.expand(&path, 2).expect("expand");
//! let _expanded = ns.render().expect("render with expansion");
//!
//! // Validate invariants.
//! ns.validate().expect("invariants hold");
//!
//! // Snapshot and restore.
//! let snapshot = ns.snapshot().expect("snapshot");
//! let (restored, _receipt) = ContextNamespace::restore(
//!     snapshot.data(),
//!     RestoreOptions::default(),
//! ).expect("restore");
//! assert_eq!(restored.version(), ns.version());
//! ```

#![forbid(unsafe_code)]
#![allow(dead_code)]

pub mod config;
pub mod embedding;
pub mod error;
pub mod event;
pub mod id;
pub mod namespace;
pub mod node;
pub mod path;
pub mod policy;
pub mod receipt;
pub mod record;
pub mod render;
pub mod simple;
pub mod snapshot;
pub mod summary;
pub mod transaction;
pub mod validation;

#[cfg(feature = "async-adapters")]
pub mod adapter;

// Re-export intentional public types — added incrementally as modules are
// implemented through TDD.
pub use crate::config::{
    CarrierTemplate, DefensiveLimits, IndexMode, NamespaceConfig, PolicyConfig, RenderOptions,
};
pub use crate::error::{NamespaceError, WorkflowError};
pub use crate::event::{NamespaceEvent, StructuralStats};
pub use crate::id::{ContentHash, NodeId, RecordId, RenderId, RequestId};
pub use crate::namespace::ContextNamespace;
pub use crate::node::{
    NamespaceNode, NodeDescriptor, NodeKey, NodeSource, NodeSourceKind, OwnerLocation, Provenance,
};
pub use crate::path::{NamespacePath, NodeLabel, NodeLocator, PathError};
pub use crate::receipt::{
    AppendReceipt, CancelReceipt, ExpansionReceipt, RenderReceipt, RestoreReceipt, SummaryReceipt,
};
pub use crate::record::{NewRecord, Record, Segment};
pub use crate::render::{MaintenanceUnit, RenderPreview, RenderedHistory, SummaryPreview};
pub use crate::snapshot::{RestoreOptions, SnapshotBytes};
pub use crate::summary::{
    ChildDescriptor, MaintenanceDecision, MaintenanceInput, MaintenanceOutcome, PreparedSummary,
    RecordLocator, RecordSelection, RecordTokenEstimate, SummaryInput, SummaryResult,
    TextSelection,
};
pub use crate::validation::{InvariantViolation, ValidationReport, ValidationWarning};

#[cfg(feature = "async-adapters")]
pub use crate::adapter::Summarizer;

#[cfg(feature = "semantic-search")]
pub use crate::embedding::{
    EmbeddingBatch, EmbeddingCandidate, EmbeddingModelSpec, SearchHit, SearchOptions,
};

pub use crate::simple::{Capabilities, NodeFilter, RenderView, SimpleContextNamespace};
