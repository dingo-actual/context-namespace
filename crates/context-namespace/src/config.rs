use serde::{Deserialize, Serialize};
use std::num::{NonZeroU32, NonZeroU64};

/// Defensive ceilings for all namespace collections and outputs.
///
/// These are defensive limits, not benchmark hyperparameters. They are
/// enforced before allocation where possible and rechecked on commit/render.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DefensiveLimits {
    /// Maximum number of canonical records.
    pub max_records: NonZeroU32,
    /// Maximum number of summary nodes in the tree.
    pub max_nodes: NonZeroU32,
    /// Maximum total bytes of source material stored across all nodes.
    pub max_source_bytes: NonZeroU64,
    /// Maximum byte length of a single rendered JSON output.
    pub max_render_bytes: NonZeroU64,
    /// Maximum byte length of a single record's render field.
    pub max_record_bytes: NonZeroU64,
    /// Maximum character length of a summary text.
    pub max_summary_chars: NonZeroU32,
    /// Maximum character length of a gloss text.
    pub max_gloss_chars: NonZeroU32,
    /// Maximum depth of the summary tree.
    pub max_tree_depth: NonZeroU32,
    /// Maximum number of immediate children per node.
    pub max_children_per_node: NonZeroU32,
    /// Maximum number of simultaneously active expansions.
    pub max_active_expansions: NonZeroU32,
    /// Maximum number of pending (uncommitted) summary requests.
    pub max_pending_requests: NonZeroU32,
    /// Maximum number of retained path aliases per node.
    pub max_aliases_per_node: NonZeroU32,
    /// Number of recent render resolutions retained for stale-render detection.
    pub retained_render_resolutions: NonZeroU32,
    /// Maximum character length of a namespace label.
    pub max_label_length: NonZeroU32,
    /// Maximum number of events in the bounded event queue.
    pub max_event_queue: NonZeroU32,
}

impl Default for DefensiveLimits {
    fn default() -> Self {
        Self::portfolio_defaults()
    }
}

impl DefensiveLimits {
    /// Returns the portfolio default limits suitable for agent benchmarking.
    pub fn portfolio_defaults() -> Self {
        Self {
            max_records: NonZeroU32::new(10_000).expect("valid nonzero"),
            max_nodes: NonZeroU32::new(10_000).expect("valid nonzero"),
            max_source_bytes: NonZeroU64::new(64 * 1024 * 1024).expect("valid nonzero"),
            max_render_bytes: NonZeroU64::new(16 * 1024 * 1024).expect("valid nonzero"),
            max_record_bytes: NonZeroU64::new(256 * 1024).expect("valid nonzero"),
            max_summary_chars: NonZeroU32::new(1_200).expect("valid nonzero"),
            max_gloss_chars: NonZeroU32::new(160).expect("valid nonzero"),
            max_tree_depth: NonZeroU32::new(32).expect("valid nonzero"),
            max_children_per_node: NonZeroU32::new(64).expect("valid nonzero"),
            max_active_expansions: NonZeroU32::new(64).expect("valid nonzero"),
            max_pending_requests: NonZeroU32::new(16).expect("valid nonzero"),
            max_aliases_per_node: NonZeroU32::new(16).expect("valid nonzero"),
            retained_render_resolutions: NonZeroU32::new(8).expect("valid nonzero"),
            max_label_length: NonZeroU32::new(128).expect("valid nonzero"),
            max_event_queue: NonZeroU32::new(10_000).expect("valid nonzero"),
        }
    }
}

/// Configuration for the maintenance policy.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PolicyConfig {
    /// Effective token budget for rendered history.
    pub effective_budget_tokens: u32,
    /// Trigger threshold: summarization is considered when the next render exceeds this many tokens.
    pub trigger_tokens: u32,
    /// Target token count for the summarized range.
    pub target_tokens: u32,
    /// Number of prefix records always preserved from summarization.
    pub preserve_prefix_records: u32,
    /// Number of most-recent records always preserved from summarization.
    pub preserve_recent_records: u32,
    /// Maximum character length for summaries produced by this policy.
    pub max_summary_chars: u32,
    /// Maximum character length for glosses produced by this policy.
    pub max_gloss_chars: u32,
    /// Default TTL (in renders) for agent-requested expansions.
    pub default_expansion_ttl: NonZeroU32,
    /// Maximum TTL (in renders) for any expansion.
    pub max_expansion_ttl: NonZeroU32,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            effective_budget_tokens: 32_000,
            trigger_tokens: 24_000,
            target_tokens: 16_000,
            preserve_prefix_records: 1,
            preserve_recent_records: 8,
            max_summary_chars: 1_200,
            max_gloss_chars: 160,
            default_expansion_ttl: NonZeroU32::new(2).expect("valid nonzero"),
            max_expansion_ttl: NonZeroU32::new(4).expect("valid nonzero"),
        }
    }
}

/// Index visibility mode for rendered output.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum IndexMode {
    /// No index metadata injected.
    None,
    /// Inject the record ID only.
    RecordId,
    /// Inject the visible index only.
    VisibleIndex,
    /// Inject both the visible index and record ID.
    #[default]
    Both,
}

/// Options controlling rendered JSON output.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenderOptions {
    /// Controls which index metadata (if any) is injected into rendered records.
    pub index_mode: IndexMode,
    /// Whether to wrap rendered records in a JSON envelope object.
    pub envelope: bool,
    /// Whether to produce pretty-printed JSON.
    pub pretty: bool,
    /// The reserved metadata key used for injecting `__ctx` metadata (default: `"__ctx"`).
    pub metadata_key: String,
    /// Whether to include child gloss lists in collapsed summary markers.
    pub include_child_glosses: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            index_mode: IndexMode::Both,
            envelope: true,
            pretty: false,
            metadata_key: "__ctx".to_string(),
            include_child_glosses: true,
        }
    }
}

/// Declarative template for synthetic carrier records.
///
/// When a record range is summarized, the range is replaced by one synthetic
/// carrier record. This template controls how that record is constructed.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CarrierTemplate {
    /// The role field value for synthetic carrier records (default: `"system"`).
    pub role: String,
}

/// Top-level configuration for a [`crate::ContextNamespace`].
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NamespaceConfig {
    /// The string field name that the model sees (default: `"content"`).
    pub render_field: String,
    /// Template for synthetic carrier records created by record-range summarization.
    pub carrier: CarrierTemplate,
    /// Options controlling rendered JSON output.
    pub render: RenderOptions,
    /// Maintenance policy configuration enforced by Rust.
    pub policy: PolicyConfig,
    /// Defensive ceilings for all namespace collections and outputs.
    pub limits: DefensiveLimits,
}
