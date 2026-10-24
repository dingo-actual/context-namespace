//! High-level, easy-to-use wrapper around [`ContextNamespace`].
//!
//! [`SimpleContextNamespace`] wraps the core state machine in an
//! `Arc<Mutex<...>>` and exposes a small set of simplified methods. Each
//! method hides the low-level transaction details (prepare/commit,
//! `IndexMap` construction, `NodeLabel` validation, etc.) so that callers
//! can work with plain strings and straightforward parameters.
//!
//! For advanced use cases — custom scheduling, text-slice summarization,
//! maintenance planning with real tokenizers, or manual transaction
//! control — use [`ContextNamespace`] directly.
//!
//! # Example
//!
//! ```no_run
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! use context_namespace::{SimpleContextNamespace, NamespaceConfig, Summarizer, SummaryInput, SummaryResult};
//! use async_trait::async_trait;
//!
//! struct MySummarizer;
//!
//! #[async_trait]
//! impl Summarizer for MySummarizer {
//!     type Error = std::convert::Infallible;
//!     async fn summarize(&self, input: SummaryInput) -> Result<SummaryResult, Self::Error> {
//!         Ok(SummaryResult {
//!             summary: format!("Summarized {} chars", input.source.len()),
//!             gloss: "brief".to_string(),
//!         })
//!     }
//! }
//!
//! let mut ns = SimpleContextNamespace::new(NamespaceConfig::default())?;
//! ns.append("user", "Step 1: inspect the file.")?;
//! ns.append("user", "Step 2: run the tests.")?;
//! ns.append("user", "Step 3: fix the bug.")?;
//! ns.append("user", "Step 4: verify the fix.")?;
//!
//! let view = ns.render()?;
//!
//! let summarizer = MySummarizer;
//! let _receipt = ns.summarize_records(&view.render_id, 0, 3, "batch-1", &summarizer).await?;
//!
//! ns.expand("/batch-1", 2)?;
//! let _expanded = ns.render()?;
//! ns.collapse("/batch-1")?;
//!
//! ns.validate()?;
//!
//! let snapshot = ns.snapshot()?;
//! let _restored = SimpleContextNamespace::restore(snapshot.data(), Default::default())?;
//! # Ok(())
//! # }
//! ```

use std::collections::HashSet;
use std::sync::Arc;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[cfg(feature = "async-adapters")]
use crate::adapter::Summarizer;
use crate::config::NamespaceConfig;
use crate::error::NamespaceError;
#[cfg(feature = "async-adapters")]
use crate::error::WorkflowError;
use crate::event::StructuralStats;
use crate::id::RenderId;
use crate::namespace::ContextNamespace;
use crate::node::NodeDescriptor;
use crate::path::NamespacePath;
#[cfg(feature = "async-adapters")]
use crate::path::NodeLabel;
#[cfg(feature = "async-adapters")]
use crate::receipt::SummaryReceipt;
use crate::receipt::{AppendReceipt, ExpansionReceipt};
use crate::render::RenderedHistory;
use crate::snapshot::{RestoreOptions, SNAPSHOT_SCHEMA_VERSION, SnapshotBytes};
#[cfg(feature = "async-adapters")]
use crate::summary::RecordSelection;
use crate::validation::{InvariantViolation, ValidationReport};

/// Policy schema version.
const POLICY_SCHEMA_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Capabilities
// ---------------------------------------------------------------------------

/// Reports library version, schema versions, and enabled features.
///
/// Returned by [`SimpleContextNamespace::capabilities`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capabilities {
    /// The crate version from `Cargo.toml`.
    pub library_version: String,
    /// The high-level Rust API version.
    pub rust_api_version: String,
    /// Snapshot schema version.
    pub snapshot_schema: u32,
    /// Render schema version.
    pub render_schema: u32,
    /// Policy schema version.
    pub policy_schema: u32,
    /// Enabled cargo features.
    pub features: HashSet<String>,
}

impl Capabilities {
    /// Detects enabled features and schema versions at compile time.
    pub fn detect() -> Self {
        // Allowing unused mut for feature compatibility
        #[allow(unused_mut)]
        let mut features = HashSet::new();
        #[cfg(feature = "json-schema")]
        {
            features.insert("json-schema".to_string());
        }
        #[cfg(feature = "async-adapters")]
        {
            features.insert("async-adapters".to_string());
        }
        #[cfg(feature = "semantic-search")]
        {
            features.insert("semantic-search".to_string());
        }
        #[cfg(feature = "tracing")]
        {
            features.insert("tracing".to_string());
        }

        Self {
            library_version: env!("CARGO_PKG_VERSION").to_string(),
            rust_api_version: env!("CARGO_PKG_VERSION").to_string(),
            snapshot_schema: SNAPSHOT_SCHEMA_VERSION,
            render_schema: crate::namespace::RENDER_SCHEMA_VERSION,
            policy_schema: POLICY_SCHEMA_VERSION,
            features,
        }
    }
}

// ---------------------------------------------------------------------------
// NodeFilter
// ---------------------------------------------------------------------------

/// Filter for [`SimpleContextNamespace::list_nodes`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeFilter {
    /// Return all nodes.
    All,
    /// Return only nodes with an active expansion (TTL > 0).
    ExpandedOnly,
    /// Return only nodes without an active expansion.
    CollapsedOnly,
}

// ---------------------------------------------------------------------------
// RenderView
// ---------------------------------------------------------------------------

/// Simplified render result returned by [`SimpleContextNamespace::render`].
///
/// This is a re-export of [`RenderedHistory`] with the same fields. The
/// high-level API returns it directly because it is already simple enough
/// for most callers.
pub type RenderView = RenderedHistory;

// ---------------------------------------------------------------------------
// SimpleContextNamespace
// ---------------------------------------------------------------------------

/// A high-level wrapper around [`ContextNamespace`] that hides transaction
/// details behind a small set of easy-to-use methods.
///
/// The inner namespace is shared via `Arc<Mutex<...>>` so that the handle
/// can be cloned for async workflows. Each method acquires the lock, performs
/// the operation, and releases it — no lock is held across `.await` points
/// except during the summarize workflow, which releases the lock during
/// inference and reacquires it to commit.
///
/// # When to use this vs. [`ContextNamespace`]
///
/// Use `SimpleContextNamespace` when you want:
///
/// - append by `(role, content)` instead of building an `IndexMap`
/// - one-call summarize that handles prepare → inference → commit
/// - expand/collapse by path string
/// - simplified node listing with [`NodeFilter`]
/// - capabilities reporting
///
/// Use [`ContextNamespace`] directly when you need:
///
/// - manual two-phase transactions (prepare, then commit later)
/// - text-slice summarization
/// - maintenance planning with real token estimates
/// - preview renders without TTL decrement
/// - direct access to the event queue
pub struct SimpleContextNamespace {
    inner: Arc<std::sync::Mutex<ContextNamespace>>,
}

impl Clone for SimpleContextNamespace {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl SimpleContextNamespace {
    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    /// Creates an empty namespace with the given configuration.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError`] if the configuration is invalid.
    pub fn new(config: NamespaceConfig) -> Result<Self, NamespaceError> {
        let ns = ContextNamespace::new(config)?;
        Ok(Self {
            inner: Arc::new(std::sync::Mutex::new(ns)),
        })
    }

    /// Creates a namespace from a list of `(role, content)` tuples.
    ///
    /// Each tuple becomes a record with the given role and content string
    /// in the render field. The render field is taken from the config
    /// (defaults to `"content"`).
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError`] if the config is invalid or any record
    /// fails validation.
    pub fn from_simple_records(
        config: NamespaceConfig,
        records: &[(&str, &str)],
    ) -> Result<Self, NamespaceError> {
        let render_field = config.render_field.clone();
        let limits = config.limits.clone();
        let new_records: Vec<crate::record::NewRecord> = records
            .iter()
            .map(|(role, content)| {
                let mut fields = IndexMap::new();
                fields.insert("role".to_string(), serde_json::json!(*role));
                fields.insert(render_field.clone(), serde_json::json!(*content));
                crate::record::NewRecord::new(fields, &render_field, &limits)
            })
            .collect::<Result<_, _>>()?;

        let ns = ContextNamespace::from_records(config, new_records)?;
        Ok(Self {
            inner: Arc::new(std::sync::Mutex::new(ns)),
        })
    }

    // -----------------------------------------------------------------------
    // Accessors
    // -----------------------------------------------------------------------

    /// Returns the current namespace version.
    ///
    /// The version increments on every canonical mutation (append, commit).
    pub fn version(&self) -> u64 {
        self.inner.lock().unwrap().version()
    }

    /// Returns structural statistics.
    pub fn stats(&self) -> StructuralStats {
        self.inner.lock().unwrap().stats()
    }

    /// Returns the enabled features and schema versions.
    pub fn capabilities(&self) -> Capabilities {
        Capabilities::detect()
    }

    // -----------------------------------------------------------------------
    // Append
    // -----------------------------------------------------------------------

    /// Appends a record with the given role and content.
    ///
    /// This builds a `NewRecord` with `role` and `content` fields, where
    /// `content` is the render field. The render field name is taken from
    /// the namespace config.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError`] if a defensive limit is exceeded.
    pub fn append(&mut self, role: &str, content: &str) -> Result<AppendReceipt, NamespaceError> {
        let ns = self.inner.lock().unwrap();
        let render_field = ns.config().render_field.clone();
        let limits = ns.config().limits.clone();
        let mut fields = IndexMap::new();
        fields.insert("role".to_string(), serde_json::json!(role));
        fields.insert(render_field.clone(), serde_json::json!(content));
        let record = crate::record::NewRecord::new(fields, &render_field, &limits)?;
        // We need a mutable lock for append. Drop the immutable borrow
        // and reacquire. This is safe because no other thread can mutate
        // between the two locks (we hold the lock the entire time).
        drop(ns);
        let mut ns = self.inner.lock().unwrap();
        ns.append(record)
    }

    // -----------------------------------------------------------------------
    // Render
    // -----------------------------------------------------------------------

    /// Renders the current namespace state as deterministic JSON.
    ///
    /// Returns a [`RenderView`] containing the render ID (needed for
    /// summarize operations), the JSON string, byte count, and entry count.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError`] if the rendered output exceeds
    /// `max_render_bytes`.
    pub fn render(&mut self) -> Result<RenderView, NamespaceError> {
        self.inner.lock().unwrap().render()
    }

    // -----------------------------------------------------------------------
    // Summarize (high-level async)
    // -----------------------------------------------------------------------

    /// Summarizes a consecutive record range in a single awaitable call.
    ///
    /// This method:
    ///
    /// 1. Prepares a record-range summary (validates indices against the
    ///    given `render_id`).
    /// 2. Calls the summarizer to produce summary text and gloss.
    /// 3. Commits the result atomically.
    ///
    /// If the summarizer fails, the pending request is cancelled and
    /// canonical state is not mutated.
    ///
    /// # Arguments
    ///
    /// - `render_id` — the render ID from a recent [`render`](Self::render)
    ///   call. Must be fresh (no mutations since that render).
    /// - `start` — the inclusive start visible index.
    /// - `end` — the exclusive end visible index.
    /// - `label` — the path label for the new summary node (e.g. `"batch-1"`).
    /// - `summarizer` — an implementation of [`Summarizer`].
    ///
    /// # Errors
    ///
    /// Returns [`WorkflowError::Inference`] if the summarizer fails, or
    /// [`WorkflowError::Core`] for namespace errors.
    #[cfg(feature = "async-adapters")]
    pub async fn summarize_records<S>(
        &mut self,
        render_id: &RenderId,
        start: u32,
        end: u32,
        label: &str,
        summarizer: &S,
    ) -> Result<SummaryReceipt, WorkflowError<S::Error>>
    where
        S: Summarizer,
    {
        // Phase 1: prepare under lock, then release.
        let prepared = {
            let mut ns = self.inner.lock().unwrap();
            let limits = ns.config().limits.clone();
            let node_label = NodeLabel::new(label, &limits)
                .map_err(|e| WorkflowError::Core(NamespaceError::Config(e.to_string())))?;
            ns.prepare_record_summary(
                RecordSelection {
                    render_id: *render_id,
                    start_index: start,
                    end_index: end,
                },
                node_label,
            )?
        };

        // Phase 2: inference without holding the lock.
        let result = match summarizer.summarize(prepared.input.clone()).await {
            Ok(result) => result,
            Err(error) => {
                // Best-effort cleanup.
                let mut ns = self.inner.lock().unwrap();
                let _ = ns.cancel_summary(prepared.request_id);
                return Err(WorkflowError::Inference(error));
            }
        };

        // Phase 3: commit under lock.
        let mut ns = self.inner.lock().unwrap();
        ns.commit_summary(prepared, result)
            .map_err(WorkflowError::Core)
    }

    // -----------------------------------------------------------------------
    // Expand and collapse
    // -----------------------------------------------------------------------

    /// Expands a summary node by path for `ttl` successful renders.
    ///
    /// During rendering, an expanded node has its stored source text
    /// inlined in place of the gloss marker. TTLs decrement after each
    /// successful render and auto-collapse at zero.
    ///
    /// # Arguments
    ///
    /// - `path` — the absolute path of the node (e.g. `"/batch-1"`).
    /// - `ttl` — the number of renders to keep the node expanded.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError::UnknownPath`] if the path does not resolve,
    /// or [`NamespaceError::LimitExceeded`] if the TTL or active expansion
    /// count exceeds limits.
    pub fn expand(&mut self, path: &str, ttl: u32) -> Result<ExpansionReceipt, NamespaceError> {
        let mut ns = self.inner.lock().unwrap();
        let limits = ns.config().limits.clone();
        let parsed = NamespacePath::parse(path, &limits)
            .map_err(|e| NamespaceError::UnknownPath(e.to_string()))?;
        ns.expand(&parsed, ttl)
    }

    /// Clears the active expansion on a node, collapsing it immediately.
    ///
    /// Canonical state is not modified; only the expansion overlay is cleared.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError::UnknownPath`] if the path does not resolve.
    pub fn collapse(&mut self, path: &str) -> Result<ExpansionReceipt, NamespaceError> {
        let mut ns = self.inner.lock().unwrap();
        let limits = ns.config().limits.clone();
        let parsed = NamespacePath::parse(path, &limits)
            .map_err(|e| NamespaceError::UnknownPath(e.to_string()))?;
        let node_id = ns.resolve_path(&parsed)?;
        let key = ns
            .key_by_id(node_id)
            .ok_or_else(|| NamespaceError::UnknownPath(node_id.to_string()))?;
        ns.clear_expansion(key);
        Ok(ExpansionReceipt {
            node_id,
            ttl_renders: 0,
            render_id: RenderId::new(uuid::Uuid::nil()),
        })
    }

    /// Checks whether a node at the given path is currently expanded.
    pub fn is_expanded(&self, path: &str) -> bool {
        let ns = self.inner.lock().unwrap();
        let limits = ns.config().limits.clone();
        if let Ok(parsed) = NamespacePath::parse(path, &limits) {
            if let Ok(node_id) = ns.resolve_path(&parsed) {
                return ns.is_expanded(node_id);
            }
        }
        false
    }

    // -----------------------------------------------------------------------
    // List nodes
    // -----------------------------------------------------------------------

    /// Returns descriptors for nodes matching the given filter.
    pub fn list_nodes(&self, filter: NodeFilter) -> Vec<NodeDescriptor> {
        let ns = self.inner.lock().unwrap();
        let all = ns.list_nodes();
        match filter {
            NodeFilter::All => all,
            NodeFilter::ExpandedOnly => all.into_iter().filter(|n| n.expanded).collect(),
            NodeFilter::CollapsedOnly => all.into_iter().filter(|n| !n.expanded).collect(),
        }
    }

    // -----------------------------------------------------------------------
    // Validate
    // -----------------------------------------------------------------------

    /// Validates all namespace invariants.
    ///
    /// # Errors
    ///
    /// Returns [`InvariantViolation`] if any invariant is violated.
    pub fn validate(&self) -> Result<ValidationReport, InvariantViolation> {
        self.inner.lock().unwrap().validate()
    }

    // -----------------------------------------------------------------------
    // Snapshot and restore
    // -----------------------------------------------------------------------

    /// Serializes the namespace to versioned JSON with a content hash.
    pub fn snapshot(&self) -> Result<SnapshotBytes, NamespaceError> {
        self.inner.lock().unwrap().snapshot()
    }

    /// Rebuilds a namespace from snapshot bytes with full invariant
    /// validation.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError`] on version mismatch, hash failure, or
    /// invariant violation.
    pub fn restore(bytes: &[u8], options: RestoreOptions) -> Result<Self, NamespaceError> {
        let (ns, _receipt) = ContextNamespace::restore(bytes, options)?;
        Ok(Self {
            inner: Arc::new(std::sync::Mutex::new(ns)),
        })
    }
}
