//! End-to-end lifecycle tests for the low-level `ContextNamespace` API.
//!
//! These tests exercise multi-operation sequences that verify state
//! transitions, invariants, and observable outputs at each step. They
//! complement the per-module integration tests by exercising complete
//! agent-like workflows.

use context_namespace::{
    ContextNamespace, DefensiveLimits, MaintenanceDecision, MaintenanceInput, NamespaceConfig,
    NamespaceEvent, NamespacePath, NewRecord, NodeLabel, NodeLocator, RecordLocator,
    RecordSelection, RecordTokenEstimate, RestoreOptions, SummaryResult, TextSelection,
};
use indexmap::IndexMap;
use std::num::NonZeroU32;

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

fn parse_path(s: &str) -> NamespacePath {
    NamespacePath::parse(s, &DefensiveLimits::portfolio_defaults()).expect("valid path")
}

fn append_records(ns: &mut ContextNamespace, contents: &[&str]) {
    for content in contents {
        ns.append(make_record(content)).expect("append");
    }
}

/// Prepares and commits a record-range summary in one step.
fn summarize_records(
    ns: &mut ContextNamespace,
    start: u32,
    end: u32,
    label: &str,
) -> context_namespace::SummaryReceipt {
    let view = ns.render().expect("render");
    let selection = RecordSelection {
        render_id: view.render_id,
        start_index: start,
        end_index: end,
    };
    let prepared = ns
        .prepare_record_summary(selection, make_label(label))
        .expect("prepare");
    let result = SummaryResult {
        summary: format!("Summary of records {start}..{end}"),
        gloss: format!("{label} gloss"),
    };
    ns.commit_summary(prepared, result).expect("commit")
}

/// Builds a `MaintenanceInput` from a preview with uniform per-record tokens.
fn input_from_preview(
    preview: &context_namespace::RenderPreview,
    total_tokens: u32,
    tokens_per_record: u32,
) -> MaintenanceInput {
    let records = preview
        .maintenance_units
        .iter()
        .map(|unit| RecordTokenEstimate {
            record_id: unit.record_id,
            render_digest: unit.render_digest.clone(),
            estimated_tokens: tokens_per_record,
        })
        .collect();
    MaintenanceInput {
        estimated_next_tokens: total_tokens,
        records,
    }
}

// ===========================================================================
// Full lifecycle: create → append → render → summarize → nest → expand →
// render → maintenance → snapshot → restore → validate
// ===========================================================================

#[test]
fn full_lifecycle_append_summarize_nest_expand_snapshot_restore() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");

    // --- Step 1: Append 10 records ---
    for i in 0..10 {
        ns.append(make_record(&format!("record {i} details")))
            .expect("append");
    }
    assert_eq!(ns.stats().records, 10);
    assert_eq!(ns.stats().nodes, 0);
    assert_eq!(ns.version(), 10);
    ns.validate().expect("invariants after append");

    // --- Step 2: Render ---
    let view = ns.render().expect("render");
    assert_eq!(view.entries, 10);
    assert!(view.bytes > 0);
    assert_eq!(view.namespace_version, ns.version());
    ns.validate().expect("invariants after render");

    // --- Step 3: Summarize records 0..5 → /batch-1 ---
    let receipt1 = summarize_records(&mut ns, 0, 5, "batch-1");
    assert_eq!(ns.stats().nodes, 1);
    // 10 - 5 + 1 carrier = 6 records
    assert_eq!(ns.stats().records, 6);
    assert_eq!(ns.version(), 11);
    ns.validate().expect("invariants after first summary");

    // Verify the node is resolvable.
    let path1 = parse_path("/batch-1");
    assert_eq!(
        ns.resolve_path(&path1).expect("resolve"),
        receipt1.new_node_id
    );

    // --- Step 4: Render to get fresh render_id ---
    let view2 = ns.render().expect("render after first summary");
    assert_eq!(view2.entries, 6);

    // --- Step 5: Summarize records 0..2 → /batch-2 (wraps batch-1 carrier) ---
    // Visible indices: 0=record5, 1=carrier(batch-1), 2=record6, ...
    // Summarizing 0..2 creates a parent over record5 and the batch-1 carrier.
    let receipt2 = summarize_records(&mut ns, 0, 2, "batch-2");
    assert_eq!(ns.stats().nodes, 2);
    // 6 - 2 + 1 = 5 records
    assert_eq!(ns.stats().records, 5);
    ns.validate().expect("invariants after nesting");

    // batch-1 should now be a child of batch-2.
    let nested_path = parse_path("/batch-2/batch-1");
    assert_eq!(
        ns.resolve_path(&nested_path).expect("resolve nested"),
        receipt1.new_node_id
    );

    // The old path /batch-1 is no longer a current path (it's an alias).
    assert!(ns.resolve_path(&path1).is_err());

    // --- Step 6: Expand /batch-2 for 2 renders ---
    let path2 = parse_path("/batch-2");
    let exp_receipt = ns.expand(&path2, 2).expect("expand");
    assert_eq!(exp_receipt.ttl_renders, 2);
    assert!(ns.is_expanded(receipt2.new_node_id));
    ns.validate().expect("invariants after expand");

    // --- Step 7: Render with expansion active ---
    // batch-2's stored source inlines: the nested batch-1 carrier renders
    // as batch-1's collapsed marker (path-prefixed summary), then record
    // 5's original text.
    let expanded_view = ns.render().expect("render with expansion");
    assert!(
        expanded_view
            .json
            .contains(r"[summary(/batch-2/batch-1): Summary of records 0..5]\nrecord 5 details")
    );
    assert!(!expanded_view.json.contains("[expanded:"));
    // TTL decremented to 1.
    assert!(ns.is_expanded(receipt2.new_node_id));
    ns.validate().expect("invariants after expanded render");

    // --- Step 8: Render again — TTL reaches 0, expansion collapses ---
    let _view3 = ns.render().expect("render TTL decrement");
    assert!(!ns.is_expanded(receipt2.new_node_id));
    ns.validate().expect("invariants after TTL expiry");

    // --- Step 9: Maintenance planning — below trigger → NoAction ---
    let preview = ns.preview_render().expect("preview");
    let input = input_from_preview(&preview, 1_000, 200);
    let decision = ns.plan_maintenance(input).expect("plan");
    assert!(matches!(decision, MaintenanceDecision::NoAction { .. }));
    ns.validate()
        .expect("invariants after no-action maintenance");

    // --- Step 10: Snapshot ---
    let snapshot = ns.snapshot().expect("snapshot");
    assert!(!snapshot.data().is_empty());
    ns.validate().expect("invariants after snapshot");

    // --- Step 11: Restore ---
    let (restored, restore_receipt) =
        ContextNamespace::restore(snapshot.data(), RestoreOptions::default()).expect("restore");
    assert_eq!(restore_receipt.nodes, 2);
    assert_eq!(restore_receipt.records, 5);
    assert_eq!(restored.version(), ns.version());
    restored.validate().expect("restored invariants");

    // --- Step 12: Restored namespace paths match ---
    assert_eq!(
        restored.resolve_path(&path2).expect("resolve batch-2"),
        receipt2.new_node_id
    );
    assert_eq!(
        restored.resolve_path(&nested_path).expect("resolve nested"),
        receipt1.new_node_id
    );
}

// ===========================================================================
// Text-slice + record-range interaction lifecycle
// ===========================================================================

#[test]
fn lifecycle_text_slice_then_record_range_with_nesting() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(
        &mut ns,
        &[
            "investigate the parser module for bugs",
            "found three issues in tokenizer",
            "applied fix to delimiter handling",
            "ran tests and all pass now",
            "reviewing edge cases in unicode handling",
        ],
    );
    ns.validate().expect("invariants after initial append");

    // --- Step 1: Text-slice summary on record 0 ---
    let view = ns.render().expect("render");
    let text_sel = TextSelection {
        record: RecordLocator::Visible {
            render_id: view.render_id,
            index: 0,
        },
        start_char: 0,
        end_char: 20,
    };
    let prepared_text = ns
        .prepare_text_summary(text_sel, make_label("initial-investigation"))
        .expect("prepare text");
    let text_receipt = ns
        .commit_summary(
            prepared_text,
            SummaryResult {
                summary: "Investigation of parser module".to_string(),
                gloss: "parser investigation".to_string(),
            },
        )
        .expect("commit text");
    assert_eq!(ns.stats().nodes, 1);
    assert_eq!(ns.stats().records, 5); // text-slice doesn't change record count
    ns.validate().expect("invariants after text-slice summary");

    // --- Step 2: Record-range summary over records 1..4 ---
    let view2 = ns.render().expect("render");
    let range_sel = RecordSelection {
        render_id: view2.render_id,
        start_index: 1,
        end_index: 4,
    };
    let prepared_range = ns
        .prepare_record_summary(range_sel, make_label("fix-and-verify"))
        .expect("prepare range");
    let range_receipt = ns
        .commit_summary(
            prepared_range,
            SummaryResult {
                summary: "Found and fixed parser issues".to_string(),
                gloss: "fix and verify".to_string(),
            },
        )
        .expect("commit range");
    assert_eq!(ns.stats().nodes, 2);
    // 5 - 3 + 1 = 3 records
    assert_eq!(ns.stats().records, 3);
    ns.validate()
        .expect("invariants after record-range summary");

    // --- Step 3: Both nodes are resolvable ---
    let text_path = parse_path("/initial-investigation");
    let range_path = parse_path("/fix-and-verify");
    assert_eq!(
        ns.resolve_path(&text_path).expect("resolve text node"),
        text_receipt.new_node_id
    );
    assert_eq!(
        ns.resolve_path(&range_path).expect("resolve range node"),
        range_receipt.new_node_id
    );

    // --- Step 4: Expand both nodes simultaneously ---
    ns.expand(&text_path, 2).expect("expand text");
    ns.expand(&range_path, 1).expect("expand range");
    assert!(ns.is_expanded(text_receipt.new_node_id));
    assert!(ns.is_expanded(range_receipt.new_node_id));
    ns.validate().expect("invariants after dual expand");

    // --- Step 5: Render — both expanded, range TTL expires ---
    // Both nodes' stored sources inline: the text slice restores the
    // original record text, and the range carrier shows the original
    // records joined by newlines.
    let dual_view = ns.render().expect("render with both expanded");
    assert!(
        dual_view
            .json
            .contains("investigate the parser module for bugs")
    );
    assert!(dual_view.json.contains("applied fix to delimiter handling"));
    assert!(!dual_view.json.contains("[summary("));
    assert!(!dual_view.json.contains("[expanded:"));
    // range node had TTL=1, now expired
    assert!(!ns.is_expanded(range_receipt.new_node_id));
    // text node had TTL=2, now TTL=1
    assert!(ns.is_expanded(text_receipt.new_node_id));
    ns.validate().expect("invariants after dual render");

    // --- Step 6: Render again — text node expires ---
    let _view3 = ns.render().expect("render");
    assert!(!ns.is_expanded(text_receipt.new_node_id));
    ns.validate().expect("invariants after text node expiry");

    // --- Step 7: Snapshot and restore ---
    let snapshot = ns.snapshot().expect("snapshot");
    let (restored, _) =
        ContextNamespace::restore(snapshot.data(), RestoreOptions::default()).expect("restore");
    assert_eq!(restored.stats().nodes, 2);
    assert_eq!(restored.stats().records, 3);
    restored.validate().expect("restored invariants");
}

// ===========================================================================
// Event sequence lifecycle: verify events at each stage
// ===========================================================================

#[test]
fn lifecycle_events_track_full_operation_sequence() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    let _ = ns.drain_events(100); // clear initial

    // Append 4 records.
    for i in 0..4 {
        ns.append(make_record(&format!("event test {i}")))
            .expect("append");
    }
    let append_events: Vec<_> = ns
        .drain_events(100)
        .into_iter()
        .filter(|e| matches!(e, NamespaceEvent::Append { .. }))
        .collect();
    assert_eq!(append_events.len(), 4);

    // Render.
    ns.render().expect("render");
    let render_events: Vec<_> = ns
        .drain_events(100)
        .into_iter()
        .filter(|e| matches!(e, NamespaceEvent::Render { .. }))
        .collect();
    assert_eq!(render_events.len(), 1);

    // Summarize.
    let view = ns.render().expect("render for summarize");
    let _ = ns.drain_events(100);
    let prepared = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 2,
            },
            make_label("event-batch"),
        )
        .expect("prepare");
    let prepared_events: Vec<_> = ns
        .drain_events(100)
        .into_iter()
        .filter(|e| matches!(e, NamespaceEvent::SummaryPrepared { .. }))
        .collect();
    assert_eq!(prepared_events.len(), 1);

    ns.commit_summary(
        prepared,
        SummaryResult {
            summary: "S".to_string(),
            gloss: "G".to_string(),
        },
    )
    .expect("commit");
    let commit_events: Vec<_> = ns
        .drain_events(100)
        .into_iter()
        .filter(|e| matches!(e, NamespaceEvent::SummaryCommitted { .. }))
        .collect();
    assert_eq!(commit_events.len(), 1);

    // Expand.
    let path = parse_path("/event-batch");
    ns.expand(&path, 1).expect("expand");
    let expand_events: Vec<_> = ns
        .drain_events(100)
        .into_iter()
        .filter(|e| matches!(e, NamespaceEvent::ExpansionActivated { .. }))
        .collect();
    assert_eq!(expand_events.len(), 1);

    // Render — TTL expires.
    ns.render().expect("render");
    let expiry_events: Vec<_> = ns
        .drain_events(100)
        .into_iter()
        .filter(|e| matches!(e, NamespaceEvent::ExpansionExpired { .. }))
        .collect();
    assert_eq!(expiry_events.len(), 1);

    // Snapshot.
    ns.snapshot().expect("snapshot");
    let snapshot_events: Vec<_> = ns
        .drain_events(100)
        .into_iter()
        .filter(|e| matches!(e, NamespaceEvent::SnapshotSaved { .. }))
        .collect();
    assert_eq!(snapshot_events.len(), 1);

    ns.validate().expect("final invariants");
}

// ===========================================================================
// Maintenance-driven lifecycle: trigger fires, summary committed, history
// shrinks, subsequent render reflects the change
// ===========================================================================

#[test]
fn lifecycle_maintenance_trigger_summarizes_and_renders_correctly() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    // 12 records: enough for prefix/recent preservation with defaults.
    append_records(
        &mut ns,
        &[
            "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa",
            "lambda", "mu",
        ],
    );
    ns.validate().expect("invariants after append");

    // --- Step 1: Preview and build input above trigger (24_000) ---
    let preview = ns.preview_render().expect("preview");
    let input = input_from_preview(&preview, 50_000, 5_000);

    // --- Step 2: Plan maintenance — should select oldest eligible range ---
    let decision = ns.plan_maintenance(input).expect("plan");
    let prepared = match decision {
        MaintenanceDecision::Summarize(p) => p,
        MaintenanceDecision::NoAction { .. } => panic!("expected Summarize decision"),
    };

    // --- Step 3: Commit the maintenance summary ---
    let receipt = ns
        .commit_summary(
            prepared,
            SummaryResult {
                summary: "Maintenance auto-summary".to_string(),
                gloss: "auto".to_string(),
            },
        )
        .expect("commit");
    assert!(!receipt.replaced_record_ids.is_empty());
    assert_eq!(ns.stats().nodes, 1);
    // Records reduced: 12 - N + 1 carrier
    assert!(ns.stats().records < 12);
    ns.validate().expect("invariants after maintenance commit");

    // --- Step 4: Render reflects the reduced history ---
    let view = ns.render().expect("render after maintenance");
    assert!(view.entries < 12);
    assert!(view.json.contains("auto"));
    ns.validate()
        .expect("invariants after post-maintenance render");

    // --- Step 5: Second maintenance cycle — below trigger → NoAction ---
    let preview2 = ns.preview_render().expect("preview 2");
    let input2 = input_from_preview(&preview2, 1_000, 100);
    let decision2 = ns.plan_maintenance(input2).expect("plan 2");
    assert!(matches!(decision2, MaintenanceDecision::NoAction { .. }));
    ns.validate()
        .expect("invariants after second maintenance check");
}

// ===========================================================================
// Stale render rejection across lifecycle: render → mutate → stale index
// rejected
// ===========================================================================

#[test]
fn lifecycle_stale_render_rejected_after_mutation() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["a", "b", "c", "d", "e"]);

    // Render to get a valid render_id.
    let view = ns.render().expect("render");

    // Mutate: append a new record.
    ns.append(make_record("f")).expect("append");

    // Using the old render_id for summarize should fail.
    let result = ns.prepare_record_summary(
        RecordSelection {
            render_id: view.render_id,
            start_index: 0,
            end_index: 2,
        },
        make_label("stale-attempt"),
    );
    assert!(result.is_err());
    ns.validate().expect("invariants after stale rejection");

    // A fresh render should work.
    let fresh_view = ns.render().expect("fresh render");
    let prepared = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: fresh_view.render_id,
                start_index: 0,
                end_index: 2,
            },
            make_label("fresh-attempt"),
        )
        .expect("prepare with fresh render");
    ns.commit_summary(
        prepared,
        SummaryResult {
            summary: "S".to_string(),
            gloss: "G".to_string(),
        },
    )
    .expect("commit");
    ns.validate().expect("invariants after fresh commit");
}

// ===========================================================================
// Snapshot-restore-resume lifecycle: restore, then continue operating
// ===========================================================================

#[test]
fn lifecycle_restore_then_resume_operations() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["r0", "r1", "r2", "r3", "r4", "r5"]);
    summarize_records(&mut ns, 0, 3, "initial-batch");
    ns.validate().expect("invariants before snapshot");

    // Snapshot.
    let snapshot = ns.snapshot().expect("snapshot");

    // Restore into a new namespace.
    let (mut restored, _receipt) =
        ContextNamespace::restore(snapshot.data(), RestoreOptions::default()).expect("restore");
    restored.validate().expect("restored invariants");
    assert_eq!(restored.stats().nodes, 1);
    // 6 - 3 + 1 = 4 records
    assert_eq!(restored.stats().records, 4);

    // Resume operations: append more records.
    restored
        .append(make_record("r6"))
        .expect("append after restore");
    restored
        .append(make_record("r7"))
        .expect("append after restore");
    assert_eq!(restored.stats().records, 6);
    restored
        .validate()
        .expect("invariants after post-restore append");

    // Summarize again on the restored namespace.
    let view = restored.render().expect("render restored");
    let prepared = restored
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 2,
            },
            make_label("post-restore-batch"),
        )
        .expect("prepare after restore");
    restored
        .commit_summary(
            prepared,
            SummaryResult {
                summary: "Post-restore summary".to_string(),
                gloss: "post-restore".to_string(),
            },
        )
        .expect("commit after restore");
    assert_eq!(restored.stats().nodes, 2);
    restored
        .validate()
        .expect("invariants after post-restore summary");

    // Expand the initial-batch node (restored from snapshot, now nested
    // under /post-restore-batch).
    let initial_node_id = {
        let nested = parse_path("/post-restore-batch/initial-batch");
        restored
            .resolve_path(&nested)
            .expect("resolve nested initial-batch")
    };
    let post_restore_path = parse_path("/post-restore-batch");
    restored
        .expand(&post_restore_path, 2)
        .expect("expand parent");
    restored
        .expand_node(
            NodeLocator::from_id(initial_node_id),
            NonZeroU32::new(2).unwrap(),
        )
        .expect("expand restored");
    assert!(restored.is_expanded(initial_node_id));

    let expanded = restored.render().expect("render expanded restored");
    // post-restore-batch's source inlines; initial-batch is expanded too,
    // so its stored source (r0–r2) inlines recursively before record "r3".
    assert!(expanded.json.contains(r"r0\nr1\nr2\nr3"));
    assert!(!expanded.json.contains("[summary("));
    assert!(!expanded.json.contains("[expanded:"));
    restored
        .validate()
        .expect("invariants after restored expand+render");
}

// ===========================================================================
// Three-level nesting lifecycle: inner → mid → outer, verify paths and
// aliases at each level
// ===========================================================================

#[test]
fn lifecycle_three_level_nesting_paths_and_aliases() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]);

    // Level 1: summarize records 1..3 → /inner
    let inner_receipt = summarize_records(&mut ns, 1, 3, "inner");
    assert_eq!(ns.stats().nodes, 1);
    ns.validate().expect("invariants after inner");

    // Verify /inner resolves.
    let inner_path = parse_path("/inner");
    assert_eq!(
        ns.resolve_path(&inner_path).expect("resolve inner"),
        inner_receipt.new_node_id
    );

    // Level 2: summarize records 0..3 → /mid (wraps carrier for inner)
    // After first summary: a, [carrier], d, e, f, g, h, i, j → 8 records
    let mid_receipt = summarize_records(&mut ns, 0, 3, "mid");
    assert_eq!(ns.stats().nodes, 2);
    ns.validate().expect("invariants after mid");

    // /inner should now be at /mid/inner.
    let mid_inner_path = parse_path("/mid/inner");
    assert_eq!(
        ns.resolve_path(&mid_inner_path).expect("resolve mid/inner"),
        inner_receipt.new_node_id
    );
    // Old /inner is now an alias, not a current path.
    assert!(ns.resolve_path(&inner_path).is_err());

    // Level 3: summarize records 0..2 → /outer (wraps carrier for mid)
    // After second summary: [carrier-mid], e, f, g, h, i, j → 7 records
    // Visible: 0=carrier-mid, 1=e, 2=f, ...
    let _outer_receipt = summarize_records(&mut ns, 0, 2, "outer");
    assert_eq!(ns.stats().nodes, 3);
    ns.validate().expect("invariants after outer");

    // /mid should now be at /outer/mid, and /inner at /outer/mid/inner.
    let outer_mid_path = parse_path("/outer/mid");
    assert_eq!(
        ns.resolve_path(&outer_mid_path).expect("resolve outer/mid"),
        mid_receipt.new_node_id
    );
    let outer_mid_inner_path = parse_path("/outer/mid/inner");
    assert_eq!(
        ns.resolve_path(&outer_mid_inner_path)
            .expect("resolve outer/mid/inner"),
        inner_receipt.new_node_id
    );

    // Expand all three nodes from outer to inner.
    let outer_path = parse_path("/outer");
    ns.expand(&outer_path, 2).expect("expand outer");
    ns.expand(&outer_mid_path, 2).expect("expand mid");
    ns.expand(&outer_mid_inner_path, 2).expect("expand inner");
    assert!(ns.is_expanded(inner_receipt.new_node_id));
    ns.validate().expect("invariants after deep expand");

    let view = ns.render().expect("render with deep expansion");
    // outer, mid, and inner are all expanded, so the whole chain of
    // stored sources inlines recursively (a, b, c, d, e).
    assert!(view.json.contains(r"a\nb\nc\nd\ne"));
    assert!(!view.json.contains("[summary("));
    assert!(!view.json.contains("[expanded:"));
    ns.validate()
        .expect("invariants after deep expansion render");

    // Snapshot and restore the three-level tree.
    let snapshot = ns.snapshot().expect("snapshot");
    let (restored, _) =
        ContextNamespace::restore(snapshot.data(), RestoreOptions::default()).expect("restore");
    assert_eq!(restored.stats().nodes, 3);
    restored
        .validate()
        .expect("restored three-level invariants");

    // All deep paths resolve in the restored namespace.
    restored
        .resolve_path(&outer_mid_inner_path)
        .expect("restored deep path");
    restored
        .resolve_path(&outer_mid_path)
        .expect("restored mid path");
}

// ===========================================================================
// Re-expansion max(current, requested) across a lifecycle
// ===========================================================================

#[test]
fn lifecycle_re_expansion_preserves_higher_ttl_through_renders() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["a", "b", "c", "d"]);
    summarize_records(&mut ns, 0, 3, "batch");

    let path = parse_path("/batch");
    let node_id = ns.resolve_path(&path).expect("resolve");

    // Expand to TTL=3.
    ns.expand(&path, 3).expect("expand to 3");
    assert!(ns.is_expanded(node_id));

    // Render once — TTL becomes 2.
    ns.render().expect("render 1");
    assert!(ns.is_expanded(node_id));

    // Re-expand with TTL=1 — should keep max(2, 1) = 2.
    let receipt = ns.expand(&path, 1).expect("re-expand");
    assert_eq!(receipt.ttl_renders, 2);
    assert!(ns.is_expanded(node_id));

    // Render twice — TTL reaches 0 and collapses.
    ns.render().expect("render 2"); // TTL → 1
    assert!(ns.is_expanded(node_id));
    ns.render().expect("render 3"); // TTL → 0
    assert!(!ns.is_expanded(node_id));

    ns.validate()
        .expect("invariants after re-expansion lifecycle");
}

// ===========================================================================
// Cancel-then-reprepare lifecycle: cancel a pending request, then prepare
// and commit a different one
// ===========================================================================

#[test]
fn lifecycle_cancel_then_reprepare_different_range() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["a", "b", "c", "d", "e"]);
    let view = ns.render().expect("render");

    // Prepare a summary for records 0..2.
    let prepared1 = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 2,
            },
            make_label("first-attempt"),
        )
        .expect("prepare 1");
    let request_id1 = prepared1.request_id;

    // Cancel it.
    ns.cancel_summary(request_id1).expect("cancel");
    assert_eq!(ns.stats().pending_requests, 0);
    assert_eq!(ns.stats().nodes, 0);
    ns.validate().expect("invariants after cancel");

    // Prepare a different range: 1..4.
    let prepared2 = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 1,
                end_index: 4,
            },
            make_label("second-attempt"),
        )
        .expect("prepare 2");

    // Commit it.
    let receipt = ns
        .commit_summary(
            prepared2,
            SummaryResult {
                summary: "Second attempt summary".to_string(),
                gloss: "second".to_string(),
            },
        )
        .expect("commit 2");
    assert_eq!(ns.stats().nodes, 1);
    // 5 - 3 + 1 = 3 records
    assert_eq!(ns.stats().records, 3);
    ns.validate().expect("invariants after reprepare+commit");

    // The node is at /second-attempt.
    let path = parse_path("/second-attempt");
    assert_eq!(
        ns.resolve_path(&path).expect("resolve"),
        receipt.new_node_id
    );
}

// ===========================================================================
// Inspection lifecycle: list_nodes, inspect_node, current_path, resolve_path
// across a multi-summary sequence
// ===========================================================================

#[test]
fn lifecycle_inspection_after_multiple_summaries() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["a", "b", "c", "d", "e", "f", "g", "h"]);

    // Create two sibling summaries.
    let r1 = summarize_records(&mut ns, 0, 2, "s1");
    let r2 = summarize_records(&mut ns, 1, 3, "s2");
    assert_eq!(ns.stats().nodes, 2);
    ns.validate().expect("invariants after two summaries");

    // list_nodes should return both.
    let nodes = ns.list_nodes();
    assert_eq!(nodes.len(), 2);

    // Both should have correct labels and be roots (no parent).
    let labels: Vec<_> = nodes.iter().map(|n| n.label.as_str()).collect();
    assert!(labels.contains(&"s1"));
    assert!(labels.contains(&"s2"));

    // current_path should return the correct path for each node.
    let path1 = ns.current_path(r1.new_node_id).expect("current_path s1");
    assert_eq!(path1, parse_path("/s1"));
    let path2 = ns.current_path(r2.new_node_id).expect("current_path s2");
    assert_eq!(path2, parse_path("/s2"));

    // inspect_node by ID.
    let desc = ns
        .inspect_node(NodeLocator::from_id(r1.new_node_id))
        .expect("inspect s1");
    assert_eq!(desc.label, "s1");
    assert!(!desc.expanded);

    // inspect_node by path.
    let desc2 = ns
        .inspect_node(NodeLocator::from_path(parse_path("/s2")))
        .expect("inspect s2 by path");
    assert_eq!(desc2.node_id, r2.new_node_id);

    // Expand s1 and verify inspect shows it as expanded.
    ns.expand(&parse_path("/s1"), 1).expect("expand s1");
    let desc_expanded = ns
        .inspect_node(NodeLocator::from_id(r1.new_node_id))
        .expect("inspect expanded s1");
    assert!(desc_expanded.expanded);

    ns.validate().expect("invariants after inspection");
}
