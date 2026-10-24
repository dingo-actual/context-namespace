use context_namespace::{
    ContextNamespace, DefensiveLimits, NamespaceConfig, NamespacePath, NewRecord, NodeLabel,
    NodeLocator, RecordSelection, SummaryResult,
};
use indexmap::IndexMap;

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

fn append_records(ns: &mut ContextNamespace, contents: &[&str]) {
    for content in contents {
        ns.append(make_record(content)).expect("append");
    }
}

fn make_label(s: &str) -> NodeLabel {
    NodeLabel::new(s, &DefensiveLimits::portfolio_defaults()).expect("valid label")
}

/// Creates a namespace with 5 records, summarizes records 1–3 into one node,
/// and returns the namespace plus the new node ID from the receipt.
fn namespace_with_summary() -> (ContextNamespace, context_namespace::NodeId) {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["alpha", "beta", "gamma", "delta", "epsilon"]);
    let view = ns.render().expect("render");
    let prepared = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 1,
                end_index: 4,
            },
            make_label("batch1"),
        )
        .expect("prepare");
    let receipt = ns
        .commit_summary(
            prepared,
            SummaryResult {
                summary: "Summary of beta, gamma, delta".to_string(),
                gloss: "Batch 1".to_string(),
            },
        )
        .expect("commit");
    (ns, receipt.new_node_id)
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

#[test]
fn namespace_construction_validates_config() {
    let mut bad = config();
    bad.render_field = String::new();
    let result = ContextNamespace::new(bad);
    assert!(result.is_err());
}

#[test]
fn namespace_starts_empty() {
    let ns = ContextNamespace::new(config()).expect("valid config");
    let stats = ns.stats();
    assert_eq!(stats.records, 0);
    assert_eq!(stats.nodes, 0);
    assert_eq!(stats.active_expansions, 0);
}

#[test]
fn namespace_from_records_populates_initial_records() {
    let ns = ContextNamespace::from_records(
        config(),
        [make_record("a"), make_record("b"), make_record("c")],
    )
    .expect("valid");
    let stats = ns.stats();
    assert_eq!(stats.records, 3);
}

// ---------------------------------------------------------------------------
// Append
// ---------------------------------------------------------------------------

#[test]
fn append_record_increments_count() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    let rec = make_record("hello");
    let receipt = ns.append(rec).expect("append");
    assert_eq!(receipt.visible_index, 0);
    let stats = ns.stats();
    assert_eq!(stats.records, 1);
}

#[test]
fn append_multiple_records_assigns_incrementing_indices() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    let r1 = ns.append(make_record("a")).expect("append");
    let r2 = ns.append(make_record("b")).expect("append");
    let r3 = ns.append(make_record("c")).expect("append");
    assert_eq!(r1.visible_index, 0);
    assert_eq!(r2.visible_index, 1);
    assert_eq!(r3.visible_index, 2);
}

#[test]
fn append_rejects_when_max_records_exceeded() {
    let mut cfg = config();
    cfg.limits.max_records = std::num::NonZeroU32::new(2).unwrap();
    let mut ns = ContextNamespace::new(cfg).expect("valid");
    ns.append(make_record("a")).expect("append");
    ns.append(make_record("b")).expect("append");
    assert!(ns.append(make_record("c")).is_err());
}

// ---------------------------------------------------------------------------
// Version
// ---------------------------------------------------------------------------

#[test]
fn version_starts_at_zero() {
    let ns = ContextNamespace::new(config()).expect("valid");
    assert_eq!(ns.version(), 0);
}

#[test]
fn version_increments_on_append() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    assert_eq!(ns.version(), 0);
    ns.append(make_record("a")).expect("append");
    assert_eq!(ns.version(), 1);
    ns.append(make_record("b")).expect("append");
    assert_eq!(ns.version(), 2);
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

#[test]
fn render_empty_namespace_produces_empty_array() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    let receipt = ns.render().expect("render");
    assert_eq!(receipt.entries, 0);
}

#[test]
fn render_single_record_produces_one_entry() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    ns.append(make_record("hello world")).expect("append");
    let receipt = ns.render().expect("render");
    assert_eq!(receipt.entries, 1);
    assert!(receipt.bytes > 0);
}

#[test]
fn render_assigns_new_render_id_each_time() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    ns.append(make_record("a")).expect("append");
    let r1 = ns.render().expect("render");
    let r2 = ns.render().expect("render");
    assert_ne!(r1.render_id, r2.render_id);
}

#[test]
fn render_produces_json_envelope_by_default() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    ns.append(make_record("test")).expect("append");
    let rendered = ns.render().expect("render");
    let parsed: serde_json::Value = serde_json::from_str(&rendered.json).expect("valid json");
    assert!(parsed.is_object());
    assert!(parsed.get("schema_version").is_some());
    assert!(parsed.get("render_id").is_some());
    assert!(parsed.get("records").is_some());
}

// ---------------------------------------------------------------------------
// Node inspection (no nodes yet, verify empty state)
// ---------------------------------------------------------------------------

#[test]
fn list_nodes_returns_empty_when_no_summaries() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    append_records(&mut ns, &["a", "b", "c"]);
    let nodes = ns.list_nodes();
    assert!(nodes.is_empty());
}

#[test]
fn resolve_unknown_path_returns_error() {
    let ns = ContextNamespace::new(config()).expect("valid");
    let path = NamespacePath::parse("/nonexistent", &DefensiveLimits::portfolio_defaults())
        .expect("valid path");
    let result = ns.resolve_path(&path);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Stats: enriched fields
// ---------------------------------------------------------------------------

#[test]
fn stats_reports_zero_for_empty_namespace() {
    let ns = ContextNamespace::new(config()).expect("valid");
    let stats = ns.stats();
    assert_eq!(stats.records, 0);
    assert_eq!(stats.nodes, 0);
    assert_eq!(stats.active_expansions, 0);
    assert_eq!(stats.pending_requests, 0);
    assert_eq!(stats.alias_count, 0);
    assert_eq!(stats.max_depth, 0);
}

#[test]
fn stats_reports_records_and_nodes_after_summary() {
    let (ns, _) = namespace_with_summary();
    let stats = ns.stats();
    // 5 records - 3 summarized + 1 carrier = 3 records
    assert_eq!(stats.records, 3);
    assert_eq!(stats.nodes, 1);
    assert_eq!(stats.max_depth, 1);
}

#[test]
fn stats_reports_pending_requests_after_prepare() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["alpha", "beta", "gamma"]);
    let view = ns.render().expect("render");
    let _ = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 2,
            },
            make_label("batch1"),
        )
        .expect("prepare");
    let stats = ns.stats();
    assert_eq!(stats.pending_requests, 1);
}

#[test]
fn stats_reports_alias_count_after_reparenting() {
    let (ns, _) = namespace_with_summary();
    let stats = ns.stats();
    assert_eq!(stats.alias_count, 0);
}

#[test]
fn stats_reports_active_expansions_after_expand() {
    let (mut ns, node_id) = namespace_with_summary();
    let path = ns.current_path(node_id).expect("path exists");
    ns.expand(&path, 2).expect("expand");
    let stats = ns.stats();
    assert_eq!(stats.active_expansions, 1);
}

// ---------------------------------------------------------------------------
// list_nodes: populated tree
// ---------------------------------------------------------------------------

#[test]
fn list_nodes_returns_all_nodes_after_summary() {
    let (ns, node_id) = namespace_with_summary();
    let nodes = ns.list_nodes();
    assert_eq!(nodes.len(), 1);
    let desc = &nodes[0];
    assert_eq!(desc.node_id, node_id);
    assert_eq!(desc.label, "batch1");
    assert!(desc.parent_id.is_none());
    assert_eq!(desc.child_count, 0);
    assert!(!desc.expanded);
}

#[test]
fn list_nodes_reports_correct_paths_for_nested_summaries() {
    let (mut ns, parent_id) = namespace_with_summary();
    let view = ns.render().expect("render");
    let prepared = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 3,
            },
            make_label("root-summary"),
        )
        .expect("prepare");
    let receipt = ns
        .commit_summary(
            prepared,
            SummaryResult {
                summary: "Root summary".to_string(),
                gloss: "Root".to_string(),
            },
        )
        .expect("commit");
    let new_parent_id = receipt.new_node_id;

    let nodes = ns.list_nodes();
    assert_eq!(nodes.len(), 2);

    let parent_desc = nodes
        .iter()
        .find(|d| d.node_id == new_parent_id)
        .expect("parent found");
    assert_eq!(parent_desc.child_count, 1);

    let child_desc = nodes
        .iter()
        .find(|d| d.node_id == parent_id)
        .expect("child found");
    assert_eq!(child_desc.parent_id, Some(new_parent_id));
}

// ---------------------------------------------------------------------------
// inspect_node: by path and by ID
// ---------------------------------------------------------------------------

#[test]
fn inspect_node_by_path_returns_descriptor() {
    let (ns, node_id) = namespace_with_summary();
    let path = ns.current_path(node_id).expect("path");
    let desc = ns
        .inspect_node(NodeLocator::from_path(path))
        .expect("inspect");
    assert_eq!(desc.node_id, node_id);
    assert_eq!(desc.label, "batch1");
    assert_eq!(desc.summary, "Summary of beta, gamma, delta");
    assert_eq!(desc.gloss, "Batch 1");
    assert!(desc.parent_id.is_none());
    assert_eq!(desc.child_count, 0);
    assert!(!desc.expanded);
}

#[test]
fn inspect_node_by_id_returns_descriptor() {
    let (ns, node_id) = namespace_with_summary();
    let desc = ns
        .inspect_node(NodeLocator::from_id(node_id))
        .expect("inspect");
    assert_eq!(desc.node_id, node_id);
    assert_eq!(desc.label, "batch1");
}

#[test]
fn inspect_node_unknown_path_returns_error() {
    let ns = ContextNamespace::new(config()).expect("valid");
    let path = NamespacePath::parse("/missing", &DefensiveLimits::portfolio_defaults())
        .expect("valid path");
    let result = ns.inspect_node(NodeLocator::from_path(path));
    assert!(result.is_err());
}

#[test]
fn inspect_node_unknown_id_returns_error() {
    let ns = ContextNamespace::new(config()).expect("valid");
    let unknown = context_namespace::NodeId::new(uuid::Uuid::nil());
    let result = ns.inspect_node(NodeLocator::from_id(unknown));
    assert!(result.is_err());
}

#[test]
fn inspect_node_reports_expanded_status() {
    let (mut ns, node_id) = namespace_with_summary();
    let path = ns.current_path(node_id).expect("path");
    ns.expand(&path, 2).expect("expand");
    let desc = ns
        .inspect_node(NodeLocator::from_id(node_id))
        .expect("inspect");
    assert!(desc.expanded);
}

// ---------------------------------------------------------------------------
// resolve_path: populated tree
// ---------------------------------------------------------------------------

#[test]
fn resolve_path_returns_node_id_after_summary() {
    let (ns, node_id) = namespace_with_summary();
    let path = NamespacePath::parse("/batch1", &DefensiveLimits::portfolio_defaults())
        .expect("valid path");
    let resolved = ns.resolve_path(&path).expect("resolve");
    assert_eq!(resolved, node_id);
}

#[test]
fn resolve_path_for_child_returns_node_id() {
    let (mut ns, _parent_id) = namespace_with_summary();
    let view = ns.render().expect("render");
    let prepared = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 3,
            },
            make_label("root-summary"),
        )
        .expect("prepare");
    let receipt = ns
        .commit_summary(
            prepared,
            SummaryResult {
                summary: "Root".to_string(),
                gloss: "R".to_string(),
            },
        )
        .expect("commit");
    let child_path = ns.current_path(receipt.new_node_id).expect("path");
    let resolved = ns.resolve_path(&child_path).expect("resolve");
    assert_eq!(resolved, receipt.new_node_id);
}

// ---------------------------------------------------------------------------
// current_path: populated tree
// ---------------------------------------------------------------------------

#[test]
fn current_path_returns_path_after_summary() {
    let (ns, node_id) = namespace_with_summary();
    let path = ns.current_path(node_id).expect("current path");
    assert_eq!(path.to_path_string(), "/batch1");
}

#[test]
fn current_path_unknown_id_returns_error() {
    let ns = ContextNamespace::new(config()).expect("valid");
    let unknown = context_namespace::NodeId::new(uuid::Uuid::nil());
    let result = ns.current_path(unknown);
    assert!(result.is_err());
}

#[test]
fn current_path_updates_after_reparenting() {
    let (mut ns, parent_id) = namespace_with_summary();
    let view = ns.render().expect("render");
    let prepared = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 3,
            },
            make_label("root-summary"),
        )
        .expect("prepare");
    let receipt = ns
        .commit_summary(
            prepared,
            SummaryResult {
                summary: "Root".to_string(),
                gloss: "R".to_string(),
            },
        )
        .expect("commit");
    let new_parent_id = receipt.new_node_id;
    let child_path = ns.current_path(parent_id).expect("path");
    let parent_path = ns.current_path(new_parent_id).expect("path");
    assert!(
        child_path
            .to_path_string()
            .starts_with(&parent_path.to_path_string())
    );
}
