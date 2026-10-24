//! Integration tests for cross-component boundaries.
//!
//! These tests exercise interactions between subsystems that are not
//! covered by single-module tests or the lifecycle tests:
//! - Render overlay with nested summaries and expansions
//! - Text-slice summary followed by record-range summary
//! - Snapshot/restore preserving expansion TTL state across renders
//! - Event sequence verification for complete workflows
//! - Config interaction with limits during multi-step operations
//! - Inspect_node consistency across reparenting

use context_namespace::{
    ContextNamespace, DefensiveLimits, NamespaceConfig, NamespaceEvent, NamespacePath, NewRecord,
    NodeLabel, NodeLocator, RecordSelection, RestoreOptions, SummaryResult, TextSelection,
};
use indexmap::IndexMap;

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

fn append_records(ns: &mut ContextNamespace, contents: &[&str]) {
    for content in contents {
        ns.append(make_record(content)).expect("append");
    }
}

fn summarize_records(
    ns: &mut ContextNamespace,
    start: u32,
    end: u32,
    label: &str,
) -> context_namespace::SummaryReceipt {
    let view = ns.render().expect("render");
    let prepared = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: start,
                end_index: end,
            },
            make_label(label),
        )
        .expect("prepare");
    ns.commit_summary(
        prepared,
        SummaryResult {
            summary: format!("Summary of {label}"),
            gloss: label.to_string(),
        },
    )
    .expect("commit")
}

// ===========================================================================
// Render overlay: nested summaries with expansion
// ===========================================================================

#[test]
fn render_overlay_nested_expansion_shows_inner_content() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    append_records(&mut ns, &["a", "b", "c", "d", "e", "f"]);

    // First summary: records 0..3 → /level1
    let r1 = summarize_records(&mut ns, 0, 3, "level1");

    // Second summary: records 0..2 → /level2 (nests level1)
    let r2 = summarize_records(&mut ns, 0, 2, "level2");

    // Verify nesting: level1 is now a child of level2
    let nodes = ns.list_nodes();
    let level1_desc = nodes
        .iter()
        .find(|n| n.node_id == r1.new_node_id)
        .expect("level1 found");
    assert_eq!(level1_desc.parent_id, Some(r2.new_node_id));

    // Expand level2 (parent). The render overlay replaces the top-level
    // carrier marker with level2's stored source: the nested level1
    // carrier renders as level1's collapsed marker (path-prefixed
    // summary), then record "d".
    let path2 = ns.current_path(r2.new_node_id).expect("path2");
    ns.expand(&path2, 3).expect("expand level2");
    let view = ns.render().expect("render");
    assert!(
        view.json
            .contains(r"[summary(/level2/level1): Summary of level1]\nd"),
        "expanded source for level2 should inline at top level"
    );
    assert!(!view.json.contains("[expanded:"));

    // Expand level1 (child) as well — both should be expanded.
    let path1 = ns.current_path(r1.new_node_id).expect("path1");
    ns.expand(&path1, 3).expect("expand level1");
    assert!(ns.is_expanded(r1.new_node_id));
    assert!(ns.is_expanded(r2.new_node_id));

    // With level1 expanded as well, its stored source inlines
    // recursively, restoring records a–c inline before record "d".
    let view2 = ns.render().expect("render both expanded");
    assert!(
        view2.json.contains(r"a\nb\nc\nd"),
        "nested expanded node should inline its stored source recursively"
    );
    assert!(!view2.json.contains("[summary("));

    ns.validate().expect("invariants hold");
}

#[test]
fn render_overlay_collapsed_nested_shows_gloss() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    append_records(&mut ns, &["a", "b", "c", "d", "e", "f"]);

    // Create nested summaries without expansion.
    summarize_records(&mut ns, 0, 3, "level1");
    summarize_records(&mut ns, 0, 2, "level2");

    // Render collapsed — should show the marker with level2's summary
    // followed by a path-prefixed gloss for its immediate child level1.
    let view = ns.render().expect("render");
    assert!(
        view.json
            .contains("[summary(/level2): Summary of level2; gloss(/level2/level1): level1]"),
        "collapsed marker should show the node summary plus child glosses"
    );

    ns.validate().expect("invariants hold");
}

// ===========================================================================
// Text-slice then record-range interaction
// ===========================================================================

#[test]
fn text_slice_then_record_range_preserves_invariants() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    append_records(&mut ns, &["the quick brown fox jumps", "over the lazy dog"]);

    // Text-slice summary on first record: chars 4..15 ("quick brown")
    let view = ns.render().expect("render");
    let prepared = ns
        .prepare_text_summary(
            TextSelection {
                record: context_namespace::RecordLocator::Visible {
                    render_id: view.render_id,
                    index: 0,
                },
                start_char: 4,
                end_char: 15,
            },
            make_label("slice1"),
        )
        .expect("prepare text slice");
    ns.commit_summary(
        prepared,
        SummaryResult {
            summary: "quick brown summarized".to_string(),
            gloss: "qb".to_string(),
        },
    )
    .expect("commit text slice");

    assert_eq!(ns.stats().nodes, 1);
    ns.validate().expect("invariants after text slice");

    // Now do a record-range summary on the remaining records.
    let view2 = ns.render().expect("render");
    let prepared2 = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view2.render_id,
                start_index: 0,
                end_index: 2,
            },
            make_label("range1"),
        )
        .expect("prepare record range");
    ns.commit_summary(
        prepared2,
        SummaryResult {
            summary: "range summarized".to_string(),
            gloss: "rg".to_string(),
        },
    )
    .expect("commit record range");

    assert_eq!(ns.stats().nodes, 2);
    ns.validate().expect("invariants after both summaries");
}

#[test]
fn record_range_then_text_slice_on_different_record_preserves_invariants() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    append_records(&mut ns, &["alpha beta gamma", "delta", "epsilon"]);

    // Record-range summary on records 0..2.
    summarize_records(&mut ns, 0, 2, "batch1");
    assert_eq!(ns.stats().nodes, 1);

    // Text-slice summary on a remaining regular record (not the carrier).
    // The carrier is at index 0, the remaining records follow.
    let view = ns.render().expect("render");

    // Find a record with non-empty render text (skip the carrier).
    let json: serde_json::Value = serde_json::from_str(&view.json).expect("valid json");
    let empty: Vec<serde_json::Value> = Vec::new();
    let records = json
        .get("records")
        .and_then(|r| r.as_array())
        .unwrap_or(&empty);

    let mut target_index = None;
    for (i, rec) in records.iter().enumerate() {
        if let Some(content) = rec.get("content").and_then(|c| c.as_str()) {
            if !content.starts_with("[summary(") {
                target_index = Some(i as u32);
                break;
            }
        }
    }
    let idx = target_index.expect("must have a non-carrier record");

    let prepared = ns
        .prepare_text_summary(
            TextSelection {
                record: context_namespace::RecordLocator::Visible {
                    render_id: view.render_id,
                    index: idx,
                },
                start_char: 0,
                end_char: 3,
            },
            make_label("slice1"),
        )
        .expect("prepare text slice on remaining record");
    ns.commit_summary(
        prepared,
        SummaryResult {
            summary: "text slice summary".to_string(),
            gloss: "ts".to_string(),
        },
    )
    .expect("commit text slice");

    assert_eq!(ns.stats().nodes, 2);
    ns.validate().expect("invariants after both summary types");
}

// ===========================================================================
// Snapshot/restore preserving expansion TTL state
// ===========================================================================

#[test]
fn snapshot_restore_preserves_expansion_ttl_across_renders() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    append_records(&mut ns, &["a", "b", "c", "d"]);
    let receipt = summarize_records(&mut ns, 0, 2, "batch");

    // Expand with TTL=3.
    let path = ns.current_path(receipt.new_node_id).expect("path");
    ns.expand(&path, 3).expect("expand");

    // Render once (TTL→2).
    ns.render().expect("render");
    assert!(ns.is_expanded(receipt.new_node_id));

    // Snapshot.
    let snap = ns.snapshot().expect("snapshot");

    // Restore.
    let (restored, _) =
        ContextNamespace::restore(snap.data(), RestoreOptions::default()).expect("restore");

    // The restored namespace should still have the expansion active.
    assert_eq!(restored.stats().active_expansions, 1);
    assert!(restored.is_expanded(receipt.new_node_id));

    restored.validate().expect("restored invariants");
}

#[test]
fn snapshot_restore_preserves_multiple_expansions() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    append_records(&mut ns, &["a", "b", "c", "d", "e", "f"]);

    let r1 = summarize_records(&mut ns, 0, 2, "s1");
    let r2 = summarize_records(&mut ns, 1, 3, "s2");

    let path1 = ns.current_path(r1.new_node_id).expect("path1");
    let path2 = ns.current_path(r2.new_node_id).expect("path2");
    ns.expand(&path1, 2).expect("expand1");
    ns.expand(&path2, 3).expect("expand2");
    assert_eq!(ns.stats().active_expansions, 2);

    let snap = ns.snapshot().expect("snapshot");
    let (restored, _) =
        ContextNamespace::restore(snap.data(), RestoreOptions::default()).expect("restore");

    assert_eq!(restored.stats().active_expansions, 2);
    assert!(restored.is_expanded(r1.new_node_id));
    assert!(restored.is_expanded(r2.new_node_id));

    restored.validate().expect("restored invariants");
}

// ===========================================================================
// Event sequence verification for complete workflows
// ===========================================================================

#[test]
fn event_sequence_append_render_summarize_expand_snapshot() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    append_records(&mut ns, &["a", "b", "c", "d"]);

    // Drain initial append events.
    let _ = ns.drain_events(100);

    // Render.
    ns.render().expect("render");

    // Summarize.
    let receipt = summarize_records(&mut ns, 0, 2, "batch");

    // Expand.
    let path = ns.current_path(receipt.new_node_id).expect("path");
    ns.expand(&path, 2).expect("expand");

    // Snapshot.
    ns.snapshot().expect("snapshot");

    // Collect events.
    let events = ns.drain_events(100);

    // Verify the event sequence.
    let mut has_render = false;
    let mut has_summary_prepared = false;
    let mut has_summary_committed = false;
    let mut has_expansion_activated = false;
    let mut has_snapshot_saved = false;

    for event in &events {
        match event {
            NamespaceEvent::Render { .. } => has_render = true,
            NamespaceEvent::SummaryPrepared { .. } => has_summary_prepared = true,
            NamespaceEvent::SummaryCommitted { .. } => has_summary_committed = true,
            NamespaceEvent::ExpansionActivated { .. } => has_expansion_activated = true,
            NamespaceEvent::SnapshotSaved { .. } => has_snapshot_saved = true,
            _ => {}
        }
    }

    assert!(has_render, "must have Render event");
    assert!(has_summary_prepared, "must have SummaryPrepared event");
    assert!(has_summary_committed, "must have SummaryCommitted event");
    assert!(
        has_expansion_activated,
        "must have ExpansionActivated event"
    );
    assert!(has_snapshot_saved, "must have SnapshotSaved event");
}

#[test]
fn event_sequence_cancel_emits_cancelled_event() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    append_records(&mut ns, &["a", "b", "c"]);

    let _ = ns.drain_events(100);

    let view = ns.render().expect("render");
    let prepared = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 2,
            },
            make_label("test"),
        )
        .expect("prepare");

    ns.cancel_summary(prepared.request_id).expect("cancel");

    let events = ns.drain_events(100);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, NamespaceEvent::SummaryCancelled { .. })),
        "must have SummaryCancelled event"
    );
}

#[test]
fn event_sequence_restore_emits_restored_event() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    ns.append(make_record("a")).expect("append");
    let snap = ns.snapshot().expect("snapshot");

    let _ = ns.drain_events(100);

    let (mut restored, _) =
        ContextNamespace::restore(snap.data(), RestoreOptions::default()).expect("restore");

    // Restore emits a Restored event into the restored namespace's queue.
    let events = restored.drain_events(100);
    assert_eq!(
        events.len(),
        1,
        "restored namespace should have exactly one Restored event"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, NamespaceEvent::Restored { .. })),
        "must have Restored event"
    );
}

// ===========================================================================
// Config interaction with limits during multi-step operations
// ===========================================================================

#[test]
fn tight_limits_allow_minimal_workflow() {
    let mut cfg = config();
    cfg.limits.max_records = std::num::NonZeroU32::new(5).unwrap();
    cfg.limits.max_nodes = std::num::NonZeroU32::new(2).unwrap();
    cfg.limits.max_pending_requests = std::num::NonZeroU32::new(1).unwrap();
    let mut ns = ContextNamespace::new(cfg).expect("valid");

    // Append up to the limit.
    for i in 0..5 {
        ns.append(make_record(&format!("r{i}"))).expect("append");
    }

    // One summary is allowed (max_nodes=2).
    summarize_records(&mut ns, 0, 2, "s1");
    assert_eq!(ns.stats().nodes, 1);

    ns.validate().expect("invariants hold with tight limits");
}

#[test]
fn render_byte_limit_rejects_oversized_output() {
    let mut cfg = config();
    cfg.limits.max_render_bytes = std::num::NonZeroU64::new(10).unwrap();
    let mut ns = ContextNamespace::new(cfg).expect("valid");
    ns.append(make_record(
        "this content is way too long for the tiny render byte limit",
    ))
    .expect("append");
    let result = ns.render();
    assert!(
        result.is_err(),
        "render must fail when output exceeds byte limit"
    );
}

// ===========================================================================
// Inspect_node consistency across reparenting
// ===========================================================================

#[test]
fn inspect_node_reflects_parent_after_reparenting() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    append_records(&mut ns, &["a", "b", "c", "d", "e", "f"]);

    // Create two sibling summaries.
    let r1 = summarize_records(&mut ns, 0, 2, "s1");
    let _r2 = summarize_records(&mut ns, 1, 3, "s2");

    // Before nesting: s1 has no parent.
    let desc1 = ns
        .inspect_node(NodeLocator::from_id(r1.new_node_id))
        .expect("inspect s1");
    assert!(
        desc1.parent_id.is_none(),
        "s1 should have no parent initially"
    );

    // Create a parent summary that nests s1.
    let _r3 = summarize_records(&mut ns, 0, 2, "parent");
    assert_eq!(ns.stats().nodes, 3);

    // After nesting: s1 should have parent set.
    let desc1_after = ns
        .inspect_node(NodeLocator::from_id(r1.new_node_id))
        .expect("inspect s1 after nesting");
    assert!(
        desc1_after.parent_id.is_some(),
        "s1 should have a parent after reparenting"
    );

    ns.validate().expect("invariants after reparenting");
}

#[test]
fn inspect_node_by_path_matches_by_id_after_reparenting() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    append_records(&mut ns, &["a", "b", "c", "d", "e"]);

    let r1 = summarize_records(&mut ns, 0, 2, "child");
    summarize_records(&mut ns, 0, 2, "parent");

    // Inspect by ID.
    let by_id = ns
        .inspect_node(NodeLocator::from_id(r1.new_node_id))
        .expect("inspect by id");

    // Inspect by the current path.
    let current_path = ns.current_path(r1.new_node_id).expect("current path");
    let by_path = ns
        .inspect_node(NodeLocator::from_path(current_path))
        .expect("inspect by path");

    // Both should return the same node.
    assert_eq!(by_id.node_id, by_path.node_id);
    assert_eq!(by_id.label, by_path.label);
    assert_eq!(by_id.parent_id, by_path.parent_id);
}

// ===========================================================================
// Cross-boundary: render preview + maintenance planning
// ===========================================================================

#[test]
fn preview_render_then_plan_maintenance_completes_full_cycle() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    for i in 0..12 {
        ns.append(make_record(&format!("record {i} with content")))
            .expect("append");
    }

    // Preview render gives maintenance units.
    let preview = ns.preview_render().expect("preview");
    assert!(!preview.maintenance_units.is_empty());
    assert_eq!(preview.entries, ns.stats().records);

    // Build estimates from the preview.
    let estimates: Vec<_> = preview
        .maintenance_units
        .iter()
        .map(|u| context_namespace::RecordTokenEstimate {
            record_id: u.record_id,
            render_digest: u.render_digest.clone(),
            estimated_tokens: 5_000,
        })
        .collect();

    let maintenance_input = context_namespace::MaintenanceInput {
        estimated_next_tokens: 50_000,
        records: estimates,
    };

    // Plan maintenance — trigger threshold is 24_000, so 50_000 triggers.
    let decision = ns
        .plan_maintenance(maintenance_input)
        .expect("plan maintenance");

    match decision {
        context_namespace::MaintenanceDecision::Summarize(prepared) => {
            ns.commit_summary(
                prepared,
                SummaryResult {
                    summary: "Maintenance summary".to_string(),
                    gloss: "maint".to_string(),
                },
            )
            .expect("commit maintenance summary");
            assert!(
                ns.stats().nodes > 0,
                "maintenance should have created a node"
            );
        }
        context_namespace::MaintenanceDecision::NoAction { .. } => {
            panic!("maintenance should have been triggered at 50_000 tokens");
        }
    }

    ns.validate().expect("invariants after maintenance cycle");
}

// ===========================================================================
// Cross-boundary: from_records + snapshot + restore + resume operations
// ===========================================================================

#[test]
fn from_records_then_snapshot_restore_then_resume_append() {
    let records = vec![
        make_record("initial-1"),
        make_record("initial-2"),
        make_record("initial-3"),
    ];
    let mut ns = ContextNamespace::from_records(config(), records).expect("valid");
    assert_eq!(ns.stats().records, 3);
    assert_eq!(ns.version(), 3);

    // Snapshot.
    let snap = ns.snapshot().expect("snapshot");

    // Restore.
    let (mut restored, _) =
        ContextNamespace::restore(snap.data(), RestoreOptions::default()).expect("restore");
    assert_eq!(restored.stats().records, 3);
    assert_eq!(restored.version(), 3);

    // Resume operations on the restored namespace.
    restored
        .append(make_record("post-restore"))
        .expect("append");
    assert_eq!(restored.stats().records, 4);
    assert_eq!(restored.version(), 4);

    restored.validate().expect("invariants after resume");
}

// ===========================================================================
// Cross-boundary: expand by path, inspect by ID, collapse by ID
// ===========================================================================

#[test]
fn expand_by_path_inspect_by_id_collapse_by_id_consistent() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    append_records(&mut ns, &["a", "b", "c", "d"]);
    let receipt = summarize_records(&mut ns, 0, 2, "batch");

    // Expand by path.
    let path = ns.current_path(receipt.new_node_id).expect("path");
    ns.expand(&path, 2).expect("expand by path");

    // Inspect by ID — should show expanded.
    let desc = ns
        .inspect_node(NodeLocator::from_id(receipt.new_node_id))
        .expect("inspect by id");
    assert!(
        desc.expanded,
        "node should be expanded after expand by path"
    );

    // Collapse by ID.
    ns.collapse(receipt.new_node_id).expect("collapse by id");

    // Inspect by path — should show not expanded.
    let desc2 = ns
        .inspect_node(NodeLocator::from_path(path))
        .expect("inspect by path");
    assert!(
        !desc2.expanded,
        "node should be collapsed after collapse by id"
    );
}

// ===========================================================================
// Cross-boundary: render determinism across snapshot/restore
// ===========================================================================

#[test]
fn render_determinism_across_snapshot_restore() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    append_records(&mut ns, &["a", "b", "c", "d"]);
    summarize_records(&mut ns, 0, 2, "batch");

    let view_before = ns.render().expect("render before snapshot");
    let snap = ns.snapshot().expect("snapshot");

    let (mut restored, _) =
        ContextNamespace::restore(snap.data(), RestoreOptions::default()).expect("restore");
    let view_after = restored.render().expect("render after restore");

    // Entry count and byte count should match (render_id differs as expected).
    assert_eq!(view_before.entries, view_after.entries);
    assert_eq!(view_before.bytes, view_after.bytes);
}

// ===========================================================================
// Cross-boundary: alias path resolves after reparenting
// ===========================================================================

#[test]
fn old_path_resolves_after_reparenting() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    append_records(&mut ns, &["a", "b", "c", "d", "e", "f"]);

    // Create a node at /s1.
    let r1 = summarize_records(&mut ns, 0, 2, "s1");
    let original_path = parse_path("/s1");

    // Verify /s1 resolves to r1's node.
    assert_eq!(
        ns.resolve_path(&original_path).expect("resolve original"),
        r1.new_node_id
    );

    // Create parent that reparents s1.
    summarize_records(&mut ns, 0, 2, "parent");

    // The old path /s1 no longer resolves directly (resolve_path checks
    // current paths, not aliases). The node's current path is now /parent/s1.
    let resolved = ns.resolve_path(&original_path);
    assert!(
        resolved.is_err(),
        "old path should not resolve after reparenting (it is now an alias)"
    );

    // But the node should be reachable via its new current path.
    let new_path = ns
        .current_path(r1.new_node_id)
        .expect("current path after reparenting");
    assert_eq!(
        ns.resolve_path(&new_path).expect("resolve new path"),
        r1.new_node_id
    );

    // The alias count should reflect the retained old path.
    assert!(
        ns.stats().alias_count > 0,
        "aliases should be retained for old paths"
    );
}

// ===========================================================================
// Cross-boundary: stats consistency after multi-step workflow
// ===========================================================================

#[test]
fn stats_consistent_after_append_summarize_expand_collapse() {
    let mut ns = ContextNamespace::new(config()).expect("valid");
    append_records(&mut ns, &["a", "b", "c", "d", "e"]);

    // After append: 5 records, 0 nodes.
    assert_eq!(ns.stats().records, 5);
    assert_eq!(ns.stats().nodes, 0);
    assert_eq!(ns.stats().active_expansions, 0);

    // After summary: 3 records (5 - 2 + 1 carrier), 1 node.
    let receipt = summarize_records(&mut ns, 0, 2, "batch");
    assert_eq!(ns.stats().records, 4); // 5 - 2 + 1
    assert_eq!(ns.stats().nodes, 1);
    assert_eq!(ns.stats().active_expansions, 0);

    // After expand: 1 active expansion.
    let path = ns.current_path(receipt.new_node_id).expect("path");
    ns.expand(&path, 2).expect("expand");
    assert_eq!(ns.stats().active_expansions, 1);

    // After collapse: 0 active expansions.
    ns.collapse(receipt.new_node_id).expect("collapse");
    assert_eq!(ns.stats().active_expansions, 0);

    // Final: records and nodes unchanged by expand/collapse.
    assert_eq!(ns.stats().records, 4);
    assert_eq!(ns.stats().nodes, 1);

    ns.validate().expect("final invariants");
}
