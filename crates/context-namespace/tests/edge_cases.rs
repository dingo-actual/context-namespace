//! Edge-case and boundary-value tests for ContextNamespace.
//!
//! Covers empty-namespace operations, defensive limit boundaries,
//! and error-path coverage for all NamespaceError variants.

use context_namespace::{
    ContextNamespace, DefensiveLimits, NamespaceConfig, NamespaceError, NamespacePath, NewRecord,
    NodeLabel, NodeLocator, RecordSelection, RestoreOptions, SummaryResult,
};
use indexmap::IndexMap;
use std::num::{NonZeroU32, NonZeroU64};

// ---------------------------------------------------------------------------
// Shared fixtures
// ---------------------------------------------------------------------------

fn config() -> NamespaceConfig {
    NamespaceConfig {
        render_field: "content".to_string(),
        ..Default::default()
    }
}

fn make_record(content: &str) -> NewRecord {
    let mut fields = IndexMap::new();
    fields.insert("role".to_string(), serde_json::json!("user"));
    fields.insert("content".to_string(), serde_json::json!(content));
    NewRecord::new(fields, "content", &DefensiveLimits::portfolio_defaults()).expect("valid record")
}

fn make_label(s: &str) -> NodeLabel {
    NodeLabel::new(s, &DefensiveLimits::portfolio_defaults()).expect("valid label")
}

fn limits() -> DefensiveLimits {
    DefensiveLimits::portfolio_defaults()
}

fn parse_path(s: &str) -> NamespacePath {
    NamespacePath::parse(s, &limits()).expect("valid path")
}

// ===========================================================================
// Empty namespace operations
// ===========================================================================

#[test]
fn empty_namespace_render_produces_zero_entries() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    let receipt = ns.render().expect("render");
    assert_eq!(receipt.entries, 0);
    // With envelope enabled by default, the JSON contains a records array.
    let parsed: serde_json::Value = serde_json::from_str(&receipt.json).expect("valid json");
    assert!(parsed.get("records").is_some() || parsed.is_array());
    let empty: Vec<serde_json::Value> = Vec::new();
    let records = parsed
        .get("records")
        .and_then(|r| r.as_array())
        .unwrap_or(&empty);
    assert!(records.is_empty());
}

#[test]
fn empty_namespace_validate_succeeds() {
    let ns = ContextNamespace::new(config()).expect("valid config");
    ns.validate().expect("empty namespace is valid");
}

#[test]
#[allow(clippy::needless_borrows_for_generic_args)]
fn empty_namespace_snapshot_roundtrips() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    let snap = ns.snapshot().expect("snapshot");
    assert!(!snap.data().is_empty());
    let (restored, receipt) =
        ContextNamespace::restore(snap.data(), RestoreOptions::default()).expect("restore");
    assert_eq!(restored.version(), 0);
    assert_eq!(receipt.records, 0);
    assert_eq!(receipt.nodes, 0);
}

#[test]
fn empty_namespace_list_nodes_is_empty() {
    let ns = ContextNamespace::new(config()).expect("valid config");
    assert!(ns.list_nodes().is_empty());
}

#[test]
fn empty_namespace_stats_all_zero() {
    let ns = ContextNamespace::new(config()).expect("valid config");
    let stats = ns.stats();
    assert_eq!(stats.records, 0);
    assert_eq!(stats.nodes, 0);
    assert_eq!(stats.active_expansions, 0);
    assert_eq!(stats.pending_requests, 0);
    assert_eq!(stats.alias_count, 0);
    assert_eq!(stats.max_depth, 0);
    assert_eq!(stats.dropped_events, 0);
}

#[test]
fn empty_namespace_preview_render_produces_zero_entries() {
    let ns = ContextNamespace::new(config()).expect("valid config");
    let preview = ns.preview_render().expect("preview");
    assert_eq!(preview.entries, 0);
    assert!(preview.maintenance_units.is_empty());
}

#[test]
fn empty_namespace_drain_events_returns_empty() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    let events = ns.drain_events(100);
    assert!(events.is_empty());
}

#[test]
fn empty_namespace_resolve_path_returns_error() {
    let ns = ContextNamespace::new(config()).expect("valid config");
    let result = ns.resolve_path(&parse_path("/anything"));
    assert!(result.is_err());
}

#[test]
fn empty_namespace_inspect_node_by_path_returns_error() {
    let ns = ContextNamespace::new(config()).expect("valid config");
    let result = ns.inspect_node(NodeLocator::from_path(parse_path("/anything")));
    assert!(result.is_err());
}

#[test]
fn empty_namespace_expand_returns_error() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    let result = ns.expand(&parse_path("/anything"), 2);
    assert!(result.is_err());
}

#[test]
fn empty_namespace_collapse_returns_error() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    let result = ns.collapse(context_namespace::NodeId::new(uuid::Uuid::nil()));
    assert!(result.is_err());
}

#[test]
fn empty_namespace_prepare_record_summary_fails() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    let view = ns.render().expect("render");
    let result = ns.prepare_record_summary(
        RecordSelection {
            render_id: view.render_id,
            start_index: 0,
            end_index: 1,
        },
        make_label("test"),
    );
    assert!(result.is_err());
}

// ===========================================================================
// Defensive limit boundary conditions
// ===========================================================================

#[test]
fn max_records_boundary_at_limit_succeeds() {
    let mut cfg = config();
    cfg.limits.max_records = NonZeroU32::new(3).unwrap();
    let mut ns = ContextNamespace::new(cfg).expect("valid");
    ns.append(make_record("a")).expect("append 1");
    ns.append(make_record("b")).expect("append 2");
    ns.append(make_record("c")).expect("append 3 at boundary");
    assert_eq!(ns.stats().records, 3);
}

#[test]
fn max_records_boundary_over_limit_fails() {
    let mut cfg = config();
    cfg.limits.max_records = NonZeroU32::new(2).unwrap();
    let mut ns = ContextNamespace::new(cfg).expect("valid");
    ns.append(make_record("a")).expect("append 1");
    ns.append(make_record("b")).expect("append 2");
    assert!(ns.append(make_record("c")).is_err());
}

#[test]
fn max_record_bytes_boundary_rejects_oversized() {
    let mut tiny_limits = limits();
    tiny_limits.max_record_bytes = NonZeroU64::new(10).unwrap();
    let mut fields = IndexMap::new();
    fields.insert("content".to_string(), serde_json::json!("hello world"));
    // "hello world" is 11 bytes > 10.
    let result = NewRecord::new(fields, "content", &tiny_limits);
    assert!(result.is_err());
}

#[test]
fn max_nodes_limit_prevents_further_summaries() {
    let mut cfg = config();
    cfg.limits.max_nodes = NonZeroU32::new(1).unwrap();
    let mut ns = ContextNamespace::new(cfg).expect("valid");
    for i in 0..6 {
        ns.append(make_record(&format!("r{i}"))).expect("append");
    }
    // First summary succeeds.
    let view = ns.render().expect("render");
    let prepared = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 2,
            },
            make_label("s1"),
        )
        .expect("prepare");
    ns.commit_summary(
        prepared,
        SummaryResult {
            summary: "S1".to_string(),
            gloss: "g1".to_string(),
        },
    )
    .expect("commit");
    assert_eq!(ns.stats().nodes, 1);

    // Second summary should fail — at max_nodes.
    let view2 = ns.render().expect("render");
    let result = ns.prepare_record_summary(
        RecordSelection {
            render_id: view2.render_id,
            start_index: 0,
            end_index: 2,
        },
        make_label("s2"),
    );
    assert!(result.is_err());
}

#[test]
fn max_tree_depth_limit_prevents_deep_path() {
    let mut tiny_limits = limits();
    tiny_limits.max_tree_depth = NonZeroU32::new(1).unwrap();
    // A path at depth 1 is valid.
    assert!(NamespacePath::parse("/s1", &tiny_limits).is_ok());
    // A path at depth 2 exceeds the limit.
    assert!(NamespacePath::parse("/s1/s2", &tiny_limits).is_err());
}

#[test]
fn max_pending_requests_limit_prevents_concurrent_prepare() {
    let mut cfg = config();
    cfg.limits.max_pending_requests = NonZeroU32::new(1).unwrap();
    let mut ns = ContextNamespace::new(cfg).expect("valid");
    for i in 0..6 {
        ns.append(make_record(&format!("r{i}"))).expect("append");
    }
    let view = ns.render().expect("render");
    // First prepare succeeds.
    let _prepared1 = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 2,
            },
            make_label("s1"),
        )
        .expect("prepare 1");

    // Second prepare should fail — at max_pending_requests.
    let result = ns.prepare_record_summary(
        RecordSelection {
            render_id: view.render_id,
            start_index: 2,
            end_index: 4,
        },
        make_label("s2"),
    );
    assert!(result.is_err());
}

#[test]
fn max_label_length_boundary_rejects_oversized() {
    let mut tiny_limits = limits();
    tiny_limits.max_label_length = NonZeroU32::new(5).unwrap();
    let result = NodeLabel::new("toolong", &tiny_limits);
    assert!(result.is_err());
}

#[test]
fn max_label_length_boundary_accepts_at_limit() {
    let mut tiny_limits = limits();
    tiny_limits.max_label_length = NonZeroU32::new(5).unwrap();
    let result = NodeLabel::new("hello", &tiny_limits);
    assert!(result.is_ok());
}

// ===========================================================================
// Error-path coverage: NamespaceError variants
// ===========================================================================

#[test]
fn missing_render_field_error_when_record_lacks_field() {
    let mut fields = IndexMap::new();
    fields.insert("role".to_string(), serde_json::json!("user"));
    // NewRecord::new rejects records without the render field.
    let result = NewRecord::new(fields, "content", &limits());
    assert!(result.is_err());
}

#[test]
fn stale_render_error_when_version_changes_after_render() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    ns.append(make_record("a")).expect("append");
    let view = ns.render().expect("render");
    // Mutate the namespace (append) to bump version.
    ns.append(make_record("b")).expect("append");
    // Now prepare with stale render_id should fail.
    let result = ns.prepare_record_summary(
        RecordSelection {
            render_id: view.render_id,
            start_index: 0,
            end_index: 1,
        },
        make_label("test"),
    );
    assert!(result.is_err());
}

#[test]
fn unknown_path_error_on_nonexistent_path() {
    let ns = ContextNamespace::new(config()).expect("valid");
    let result = ns.resolve_path(&parse_path("/does-not-exist"));
    assert!(matches!(result, Err(NamespaceError::UnknownPath(_))));
}

#[test]
fn invalid_selection_error_when_range_inverted() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    ns.append(make_record("a")).expect("append");
    ns.append(make_record("b")).expect("append");
    let view = ns.render().expect("render");
    let result = ns.prepare_record_summary(
        RecordSelection {
            render_id: view.render_id,
            start_index: 1,
            end_index: 0, // inverted
        },
        make_label("test"),
    );
    assert!(result.is_err());
}

#[test]
fn invalid_selection_error_when_range_out_of_bounds() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    ns.append(make_record("a")).expect("append");
    let view = ns.render().expect("render");
    let result = ns.prepare_record_summary(
        RecordSelection {
            render_id: view.render_id,
            start_index: 0,
            end_index: 10, // out of bounds
        },
        make_label("test"),
    );
    assert!(result.is_err());
}

#[test]
fn path_collision_error_when_duplicate_label_used() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    for i in 0..6 {
        ns.append(make_record(&format!("r{i}"))).expect("append");
    }
    // First summary creates /s1.
    let view = ns.render().expect("render");
    let prepared = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 2,
            },
            make_label("s1"),
        )
        .expect("prepare");
    ns.commit_summary(
        prepared,
        SummaryResult {
            summary: "S1".to_string(),
            gloss: "g1".to_string(),
        },
    )
    .expect("commit");

    // Second summary with the same label should cause a path collision.
    let view2 = ns.render().expect("render");
    let prepared2 = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view2.render_id,
                start_index: 0,
                end_index: 2,
            },
            make_label("s1"),
        )
        .expect("prepare");
    let result = ns.commit_summary(
        prepared2,
        SummaryResult {
            summary: "S2".to_string(),
            gloss: "g2".to_string(),
        },
    );
    assert!(result.is_err());
}

#[test]
fn config_error_when_render_field_empty() {
    let mut bad = config();
    bad.render_field = String::new();
    let result = ContextNamespace::new(bad);
    assert!(matches!(result, Err(NamespaceError::Config(_))));
}

#[test]
fn config_error_when_trigger_less_than_target() {
    let mut bad = config();
    bad.policy.trigger_tokens = 10_000;
    bad.policy.target_tokens = 20_000;
    let result = ContextNamespace::new(bad);
    assert!(matches!(result, Err(NamespaceError::Config(_))));
}

#[test]
fn config_error_when_budget_less_than_trigger() {
    let mut bad = config();
    bad.policy.effective_budget_tokens = 10_000;
    bad.policy.trigger_tokens = 20_000;
    let result = ContextNamespace::new(bad);
    assert!(matches!(result, Err(NamespaceError::Config(_))));
}

#[test]
fn config_error_when_default_ttl_exceeds_max() {
    let mut bad = config();
    bad.policy.default_expansion_ttl = NonZeroU32::new(5).unwrap();
    bad.policy.max_expansion_ttl = NonZeroU32::new(4).unwrap();
    let result = ContextNamespace::new(bad);
    assert!(matches!(result, Err(NamespaceError::Config(_))));
}

#[test]
fn config_error_when_preserve_prefix_zero() {
    let mut bad = config();
    bad.policy.preserve_prefix_records = 0;
    let result = ContextNamespace::new(bad);
    assert!(matches!(result, Err(NamespaceError::Config(_))));
}

#[test]
fn config_error_when_preserve_recent_zero() {
    let mut bad = config();
    bad.policy.preserve_recent_records = 0;
    let result = ContextNamespace::new(bad);
    assert!(matches!(result, Err(NamespaceError::Config(_))));
}

#[test]
fn snapshot_version_error_for_unsupported_schema() {
    // Craft a JSON snapshot with an unsupported schema version.
    let fake_snapshot = serde_json::json!({
        "schema_version": 999,
        "library_version": "0.1.0",
        "namespace_version": 0,
        "config": {
            "render_field": "content",
            "carrier": { "role": "system", "format": "[summary: {gloss}]" },
            "render": {
                "index_mode": "Both",
                "envelope": true,
                "pretty": false,
                "metadata_key": "__ctx",
                "include_child_glosses": true,
            },
            "policy": {
                "effective_budget_tokens": 32000,
                "trigger_tokens": 24000,
                "target_tokens": 16000,
                "preserve_prefix_records": 1,
                "preserve_recent_records": 8,
                "max_summary_chars": 1200,
                "max_gloss_chars": 160,
                "default_expansion_ttl": 2,
                "max_expansion_ttl": 4,
            },
            "limits": {
                "max_records": 10000,
                "max_nodes": 1000,
                "max_source_bytes": 4194304,
                "max_render_bytes": 4194304,
                "max_record_bytes": 524288,
                "max_summary_chars": 1200,
                "max_gloss_chars": 160,
                "max_tree_depth": 64,
                "max_children_per_node": 64,
                "max_active_expansions": 16,
                "max_pending_requests": 4,
                "max_aliases_per_node": 16,
                "retained_render_resolutions": 4,
                "max_label_length": 64,
                "max_event_queue": 256,
            },
        },
        "records": [],
        "nodes": [],
        "aliases": [],
        "active_expansions": [],
        "content_sha256": "0000000000000000000000000000000000000000000000000000000000000000",
    });
    let bytes = serde_json::to_vec(&fake_snapshot).expect("serialize");
    let result = ContextNamespace::restore(&bytes, RestoreOptions::default());
    assert!(matches!(
        result,
        Err(NamespaceError::SnapshotVersion { .. })
    ));
}

#[test]
fn restore_rejects_malformed_bytes() {
    let result = ContextNamespace::restore(b"not valid json", RestoreOptions::default());
    assert!(result.is_err());
}

#[test]
fn restore_rejects_empty_bytes() {
    let result = ContextNamespace::restore(b"", RestoreOptions::default());
    assert!(result.is_err());
}

// ===========================================================================
// PathError coverage
// ===========================================================================

#[test]
fn path_error_empty_string() {
    let result = NamespacePath::parse("", &limits());
    assert!(result.is_err());
}

#[test]
fn path_error_not_absolute() {
    let result = NamespacePath::parse("relative", &limits());
    assert!(result.is_err());
}

#[test]
fn path_error_depth_exceeded() {
    let mut tiny_limits = limits();
    tiny_limits.max_tree_depth = NonZeroU32::new(1).unwrap();
    let result = NamespacePath::parse("/a/b/c", &tiny_limits);
    assert!(result.is_err());
}

#[test]
fn label_error_empty() {
    let result = NodeLabel::new("", &limits());
    assert!(result.is_err());
}

#[test]
fn label_error_control_char() {
    let result = NodeLabel::new("bad\x1flabel", &limits());
    assert!(result.is_err());
}

#[test]
fn label_error_too_long() {
    let mut tiny_limits = limits();
    tiny_limits.max_label_length = NonZeroU32::new(3).unwrap();
    let result = NodeLabel::new("toolong", &tiny_limits);
    assert!(result.is_err());
}

// ===========================================================================
// ContentHash edge cases
// ===========================================================================

#[test]
fn content_hash_from_hex_roundtrips() {
    let original = context_namespace::ContentHash::of(b"test data");
    let hex = original.to_hex();
    let restored = context_namespace::ContentHash::from_hex(&hex).expect("valid hex");
    assert_eq!(original.as_bytes(), restored.as_bytes());
}

#[test]
fn content_hash_from_hex_rejects_wrong_length() {
    let result = context_namespace::ContentHash::from_hex("abc123");
    assert!(result.is_err());
}

#[test]
fn content_hash_from_hex_rejects_invalid_chars() {
    let result = context_namespace::ContentHash::from_hex(
        "zz00000000000000000000000000000000000000000000000000000000000000",
    );
    assert!(result.is_err());
}

#[test]
fn content_hash_display_shows_hex() {
    let hash = context_namespace::ContentHash::of(b"hello");
    let display = format!("{hash}");
    assert_eq!(display.len(), 64);
    assert!(display.chars().all(|c| c.is_ascii_hexdigit()));
}

// ===========================================================================
// Version monotonically increases
// ===========================================================================

#[test]
fn version_does_not_increment_on_render() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    ns.append(make_record("a")).expect("append");
    let v0 = ns.version();
    ns.render().expect("render");
    assert_eq!(ns.version(), v0, "render must not bump version");
}

#[test]
fn version_does_not_increment_on_expand() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    for i in 0..4 {
        ns.append(make_record(&format!("r{i}"))).expect("append");
    }
    let view = ns.render().expect("render");
    let prepared = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 2,
            },
            make_label("s1"),
        )
        .expect("prepare");
    ns.commit_summary(
        prepared,
        SummaryResult {
            summary: "S1".to_string(),
            gloss: "g1".to_string(),
        },
    )
    .expect("commit");
    let v_after_commit = ns.version();
    ns.expand(&parse_path("/s1"), 2).expect("expand");
    assert_eq!(ns.version(), v_after_commit, "expand must not bump version");
}

#[test]
fn version_does_not_increment_on_validate() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    ns.append(make_record("a")).expect("append");
    let v = ns.version();
    ns.validate().expect("valid");
    assert_eq!(ns.version(), v, "validate must not bump version");
}

#[test]
fn version_does_not_increment_on_preview_render() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    ns.append(make_record("a")).expect("append");
    let v = ns.version();
    let _ = ns.preview_render();
    assert_eq!(ns.version(), v, "preview_render must not bump version");
}

#[test]
fn version_does_not_increment_on_snapshot() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    ns.append(make_record("a")).expect("append");
    let v = ns.version();
    let _ = ns.snapshot().expect("snapshot");
    assert_eq!(ns.version(), v, "snapshot must not bump version");
}

// ===========================================================================
// from_records constructor
// ===========================================================================

#[test]
fn from_records_with_empty_slice_creates_empty_namespace() {
    let ns = ContextNamespace::from_records(config(), []).expect("valid");
    assert_eq!(ns.stats().records, 0);
    assert_eq!(ns.version(), 0);
}

#[test]
fn from_records_validates_config() {
    let mut bad = config();
    bad.render_field = String::new();
    let result = ContextNamespace::from_records(bad, [make_record("a")]);
    assert!(result.is_err());
}

// ===========================================================================
// Cancel summary then re-prepare
// ===========================================================================

#[test]
fn cancel_summary_allows_new_prepare() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    for i in 0..4 {
        ns.append(make_record(&format!("r{i}"))).expect("append");
    }
    let view = ns.render().expect("render");
    let prepared = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 2,
            },
            make_label("s1"),
        )
        .expect("prepare");

    ns.cancel_summary(prepared.request_id).expect("cancel");
    assert_eq!(ns.stats().pending_requests, 0);

    // Re-prepare with the same render_id should work.
    let prepared2 = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 2,
            },
            make_label("s1"),
        )
        .expect("re-prepare");
    assert_eq!(ns.stats().pending_requests, 1);
    // Clean up.
    ns.cancel_summary(prepared2.request_id).expect("cancel");
}

// ===========================================================================
// Render with pretty option
// ===========================================================================

#[test]
fn render_with_pretty_option_produces_indented_json() {
    let mut cfg = config();
    cfg.render.pretty = true;
    let mut ns = ContextNamespace::new(cfg).expect("valid");
    ns.append(make_record("test")).expect("append");
    let receipt = ns.render().expect("render");
    assert!(receipt.json.contains("\n  ") || receipt.json.contains("\n    "));
}

#[test]
fn render_without_envelope_produces_bare_array() {
    let mut cfg = config();
    cfg.render.envelope = false;
    let mut ns = ContextNamespace::new(cfg).expect("valid");
    ns.append(make_record("test")).expect("append");
    let receipt = ns.render().expect("render");
    assert!(receipt.json.starts_with('['));
}

// ===========================================================================
// IndexMode variations
// ===========================================================================

#[test]
fn render_with_index_mode_none_omits_metadata() {
    let mut cfg = config();
    cfg.render.index_mode = context_namespace::IndexMode::None;
    let mut ns = ContextNamespace::new(cfg).expect("valid");
    ns.append(make_record("test")).expect("append");
    let receipt = ns.render().expect("render");
    // The __ctx metadata key should not be present.
    assert!(!receipt.json.contains("__ctx"));
    assert!(!receipt.json.contains("\"index\""));
    assert!(!receipt.json.contains("\"record_id\""));
}

#[test]
fn render_with_index_mode_visible_index_includes_index() {
    let mut cfg = config();
    cfg.render.index_mode = context_namespace::IndexMode::VisibleIndex;
    let mut ns = ContextNamespace::new(cfg).expect("valid");
    ns.append(make_record("test")).expect("append");
    let receipt = ns.render().expect("render");
    assert!(receipt.json.contains("\"index\""));
}
