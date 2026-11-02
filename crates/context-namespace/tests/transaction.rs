use context_namespace::{
    ContextNamespace, DefensiveLimits, NamespaceConfig, NamespacePath, NewRecord, NodeLabel,
    RecordLocator, RecordSelection, SummaryResult, TextSelection,
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

fn append_records(ns: &mut ContextNamespace, contents: &[&str]) {
    for content in contents {
        ns.append(make_record(content)).expect("append");
    }
}

// ===========================================================================
// Record-range summary transaction tests
// ===========================================================================

// ---------------------------------------------------------------------------
// Prepare: valid range
// ---------------------------------------------------------------------------

#[test]
fn prepare_record_summary_valid_range_returns_prepared() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["alpha", "beta", "gamma", "delta", "epsilon"]);
    let view = ns.render().expect("render");

    let label = make_label("batch1");
    let selection = RecordSelection {
        render_id: view.render_id,
        start_index: 1,
        end_index: 3,
    };
    let prepared = ns
        .prepare_record_summary(selection, label)
        .expect("prepare");
    assert!(!prepared.input.source.is_empty());
    assert_eq!(prepared.input.children.len(), 0);
}

#[test]
fn prepare_record_summary_source_uses_collapsed_marker_for_carriers() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["alpha", "beta", "gamma"]);

    // First commit: summarize records 0..2 → /batch.
    let view = ns.render().expect("render");
    let prepared = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 2,
            },
            make_label("batch"),
        )
        .expect("prepare first");
    ns.commit_summary(
        prepared,
        SummaryResult {
            summary: "Alpha beta".to_string(),
            gloss: "ab".to_string(),
        },
    )
    .expect("commit first");

    // Prepare a range that includes the carrier for /batch: the nested
    // reference must appear in the extracted source exactly as a
    // collapsed render would show it.
    let view2 = ns.render().expect("render");
    let prepared2 = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view2.render_id,
                start_index: 0,
                end_index: 2,
            },
            make_label("outer"),
        )
        .expect("prepare second");
    // Canonical sequence is [carrier, "gamma"]; the carrier is the
    // collapsed marker for /batch, then "gamma" follows on a new line.
    assert_eq!(
        prepared2.input.source,
        "[summary(/batch): Alpha beta]\ngamma"
    );
    assert_eq!(prepared2.input.children.len(), 1);
}

// ---------------------------------------------------------------------------
// Prepare: stale render ID
// ---------------------------------------------------------------------------

#[test]
fn prepare_record_summary_stale_render_id_returns_error() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["alpha", "beta", "gamma"]);
    let view = ns.render().expect("render");

    // Append a record after render — this bumps version and invalidates.
    ns.append(make_record("delta")).expect("append");

    let label = make_label("batch1");
    let selection = RecordSelection {
        render_id: view.render_id,
        start_index: 0,
        end_index: 2,
    };
    let result = ns.prepare_record_summary(selection, label);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Prepare: invalid range (start >= end)
// ---------------------------------------------------------------------------

#[test]
fn prepare_record_summary_start_ge_end_returns_error() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["alpha", "beta", "gamma"]);
    let view = ns.render().expect("render");

    let label = make_label("batch1");
    let selection = RecordSelection {
        render_id: view.render_id,
        start_index: 2,
        end_index: 2,
    };
    let result = ns.prepare_record_summary(selection, label);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Prepare: end index out of bounds
// ---------------------------------------------------------------------------

#[test]
fn prepare_record_summary_end_out_of_bounds_returns_error() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["alpha", "beta"]);
    let view = ns.render().expect("render");

    let label = make_label("batch1");
    let selection = RecordSelection {
        render_id: view.render_id,
        start_index: 0,
        end_index: 10,
    };
    let result = ns.prepare_record_summary(selection, label);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Commit: valid → creates node, replaces records with carrier
// ---------------------------------------------------------------------------

#[test]
fn commit_record_summary_valid_creates_node_and_replaces_records() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["alpha", "beta", "gamma", "delta", "epsilon"]);
    let view = ns.render().expect("render");

    let label = make_label("batch1");
    let selection = RecordSelection {
        render_id: view.render_id,
        start_index: 1,
        end_index: 4,
    };
    let prepared = ns
        .prepare_record_summary(selection, label)
        .expect("prepare");

    let result = SummaryResult {
        summary: "Beta through delta summarized".to_string(),
        gloss: "batch summary".to_string(),
    };
    let receipt = ns.commit_summary(prepared, result).expect("commit");

    // Node was created.
    assert_eq!(ns.stats().nodes, 1);

    // Records changed: 5 - 3 removed + 1 carrier = 3 records.
    assert_eq!(ns.stats().records, 3);

    // The new node is resolvable by path.
    let path = NamespacePath::parse("/batch1", &DefensiveLimits::portfolio_defaults())
        .expect("valid path");
    assert!(ns.resolve_path(&path).is_ok());

    // The receipt has the new node ID.
    assert_eq!(
        receipt.new_node_id,
        ns.resolve_path(&path).expect("resolve")
    );
}

// ---------------------------------------------------------------------------
// Commit: stale (version changed) → error, state unchanged
// ---------------------------------------------------------------------------

#[test]
fn commit_record_summary_stale_version_returns_error_and_state_unchanged() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["alpha", "beta", "gamma", "delta"]);
    let view = ns.render().expect("render");

    let label = make_label("batch1");
    let selection = RecordSelection {
        render_id: view.render_id,
        start_index: 1,
        end_index: 3,
    };
    let prepared = ns
        .prepare_record_summary(selection, label)
        .expect("prepare");

    // Mutate the namespace after prepare — makes the request stale.
    ns.append(make_record("epsilon")).expect("append");

    let result = SummaryResult {
        summary: "Summary".to_string(),
        gloss: "gloss".to_string(),
    };
    let err = ns.commit_summary(prepared, result);
    assert!(err.is_err());

    // State is unchanged: no nodes, 5 records.
    assert_eq!(ns.stats().nodes, 0);
    assert_eq!(ns.stats().records, 5);
}

// ---------------------------------------------------------------------------
// Cancel: pending removed, state unchanged
// ---------------------------------------------------------------------------

#[test]
fn cancel_summary_removes_pending_and_leaves_state_unchanged() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["alpha", "beta", "gamma"]);
    let view = ns.render().expect("render");

    let label = make_label("batch1");
    let selection = RecordSelection {
        render_id: view.render_id,
        start_index: 0,
        end_index: 2,
    };
    let prepared = ns
        .prepare_record_summary(selection, label)
        .expect("prepare");
    let request_id = prepared.request_id;

    let cancel_receipt = ns.cancel_summary(request_id).expect("cancel");
    assert_eq!(cancel_receipt.request_id, request_id.to_string());

    // State unchanged.
    assert_eq!(ns.stats().nodes, 0);
    assert_eq!(ns.stats().records, 3);

    // The pending request is gone — cancel again fails.
    assert!(ns.cancel_summary(request_id).is_err());
}

// ---------------------------------------------------------------------------
// Commit: range containing carrier records → child nodes reparented
// ---------------------------------------------------------------------------

#[test]
fn commit_record_summary_range_with_carriers_reparents_children() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(
        &mut ns,
        &["alpha", "beta", "gamma", "delta", "epsilon", "zeta"],
    );

    // First summary: records 1..3 → creates node at /inner.
    let view1 = ns.render().expect("render");
    let label1 = make_label("inner");
    let sel1 = RecordSelection {
        render_id: view1.render_id,
        start_index: 1,
        end_index: 3,
    };
    let prepared1 = ns.prepare_record_summary(sel1, label1).expect("prepare");
    let result1 = SummaryResult {
        summary: "Inner summary".to_string(),
        gloss: "inner".to_string(),
    };
    let receipt1 = ns.commit_summary(prepared1, result1).expect("commit");

    // Now we have: alpha, [carrier for inner], delta, epsilon, zeta
    // 6 originals - 2 removed + 1 carrier = 5 records.
    assert_eq!(ns.stats().records, 5);
    assert_eq!(ns.stats().nodes, 1);

    // Second summary: records 1..4 → wraps the carrier and surrounding records.
    // Visible indices: 0=alpha, 1=carrier, 2=delta, 3=epsilon, 4=zeta
    let view2 = ns.render().expect("render");
    let label2 = make_label("outer");
    let sel2 = RecordSelection {
        render_id: view2.render_id,
        start_index: 1,
        end_index: 4,
    };
    let prepared2 = ns.prepare_record_summary(sel2, label2).expect("prepare");

    // The prepared summary should have one child (the inner node).
    assert_eq!(prepared2.input.children.len(), 1);
    assert_eq!(prepared2.input.children[0].node_id, receipt1.new_node_id);

    let result2 = SummaryResult {
        summary: "Outer summary".to_string(),
        gloss: "outer".to_string(),
    };
    let _receipt2 = ns.commit_summary(prepared2, result2).expect("commit");

    // Two nodes now.
    assert_eq!(ns.stats().nodes, 2);

    // The inner node should be a child of the outer node.
    let inner_path = NamespacePath::parse("/outer/inner", &DefensiveLimits::portfolio_defaults())
        .expect("valid path");
    assert!(ns.resolve_path(&inner_path).is_ok());

    // The old path /inner should now be an alias.
    let old_inner_path =
        NamespacePath::parse("/inner", &DefensiveLimits::portfolio_defaults()).expect("valid path");
    // resolve_path checks current paths only, not aliases. So it should fail
    // since the inner node moved to /outer/inner.
    assert!(ns.resolve_path(&old_inner_path).is_err());
}

// ---------------------------------------------------------------------------
// Commit: summary result too long → error
// ---------------------------------------------------------------------------

#[test]
fn commit_record_summary_result_too_long_returns_error() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["alpha", "beta", "gamma"]);
    let view = ns.render().expect("render");

    let label = make_label("batch1");
    let selection = RecordSelection {
        render_id: view.render_id,
        start_index: 0,
        end_index: 2,
    };
    let prepared = ns
        .prepare_record_summary(selection, label)
        .expect("prepare");

    // The default max_summary_chars is 1200. Create a summary that exceeds it.
    let too_long = "x".repeat(1201);
    let result = SummaryResult {
        summary: too_long,
        gloss: "ok".to_string(),
    };
    let err = ns.commit_summary(prepared, result);
    assert!(err.is_err());

    // State unchanged.
    assert_eq!(ns.stats().nodes, 0);
    assert_eq!(ns.stats().records, 3);
}

// ---------------------------------------------------------------------------
// Rejected commits release the pending request
// ---------------------------------------------------------------------------

#[test]
fn rejected_commit_releases_pending_request_and_valid_retry_succeeds() {
    let mut cfg = config();
    cfg.limits.max_pending_requests = NonZeroU32::new(1).unwrap();
    cfg.limits.max_summary_chars = NonZeroU32::new(4).unwrap();
    let mut ns = ContextNamespace::new(cfg).expect("valid config");
    append_records(&mut ns, &["alpha", "beta", "gamma"]);

    let view = ns.render().expect("render");
    let prepared = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 2,
            },
            make_label("batch"),
        )
        .expect("prepare");

    // The summary exceeds max_summary_chars, so the commit is rejected.
    let err = ns.commit_summary(
        prepared,
        SummaryResult {
            summary: "12345".to_string(),
            gloss: "g".to_string(),
        },
    );
    assert!(err.is_err());
    assert_eq!(ns.stats().pending_requests, 0);

    // The single pending slot is free again, so a valid retry succeeds.
    let view = ns.render().expect("render");
    let prepared = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 2,
            },
            make_label("batch"),
        )
        .expect("retry prepare");
    ns.commit_summary(
        prepared,
        SummaryResult {
            summary: "ok".to_string(),
            gloss: "g".to_string(),
        },
    )
    .expect("retry commit");
    assert_eq!(ns.stats().nodes, 1);
    assert_eq!(ns.stats().pending_requests, 0);
    ns.validate().expect("invariants");
}

#[test]
fn stale_commit_releases_pending_request_and_valid_retry_succeeds() {
    let mut cfg = config();
    cfg.limits.max_pending_requests = NonZeroU32::new(1).unwrap();
    let mut ns = ContextNamespace::new(cfg).expect("valid config");
    append_records(&mut ns, &["alpha", "beta", "gamma"]);

    let view = ns.render().expect("render");
    let prepared = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 2,
            },
            make_label("batch"),
        )
        .expect("prepare");

    // An append while the request is pending makes the commit stale.
    ns.append(make_record("delta")).expect("append");
    let err = ns.commit_summary(
        prepared,
        SummaryResult {
            summary: "ok".to_string(),
            gloss: "g".to_string(),
        },
    );
    assert!(err.is_err());
    assert_eq!(ns.stats().pending_requests, 0);

    let view = ns.render().expect("render");
    let prepared = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 2,
            },
            make_label("batch"),
        )
        .expect("retry prepare");
    ns.commit_summary(
        prepared,
        SummaryResult {
            summary: "ok".to_string(),
            gloss: "g".to_string(),
        },
    )
    .expect("retry commit");
    assert_eq!(ns.stats().nodes, 1);
    assert_eq!(ns.stats().pending_requests, 0);
    ns.validate().expect("invariants");
}

#[test]
fn failed_mutation_planning_releases_pending_request() {
    let mut cfg = config();
    cfg.limits.max_pending_requests = NonZeroU32::new(1).unwrap();
    let mut ns = ContextNamespace::new(cfg).expect("valid config");
    append_records(&mut ns, &["alpha", "beta", "gamma", "delta"]);

    // Occupy /batch.
    let view = ns.render().expect("render");
    let prepared = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 1,
            },
            make_label("batch"),
        )
        .expect("prepare");
    ns.commit_summary(
        prepared,
        SummaryResult {
            summary: "ok".to_string(),
            gloss: "g".to_string(),
        },
    )
    .expect("commit");

    // The same label collides during mutation planning.
    let view = ns.render().expect("render");
    let prepared = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 1,
                end_index: 3,
            },
            make_label("batch"),
        )
        .expect("prepare colliding");
    let err = ns.commit_summary(
        prepared,
        SummaryResult {
            summary: "ok".to_string(),
            gloss: "g".to_string(),
        },
    );
    assert!(err.is_err());
    assert_eq!(ns.stats().pending_requests, 0);

    // The freed slot allows a differently labelled summary.
    let prepared = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 1,
                end_index: 3,
            },
            make_label("other"),
        )
        .expect("retry prepare");
    ns.commit_summary(
        prepared,
        SummaryResult {
            summary: "ok".to_string(),
            gloss: "g".to_string(),
        },
    )
    .expect("retry commit");
    assert_eq!(ns.stats().pending_requests, 0);
    ns.validate().expect("invariants");
}

// ---------------------------------------------------------------------------
// Prepare: pending limit exceeded
// ---------------------------------------------------------------------------

#[test]
fn prepare_record_summary_pending_limit_exceeded_returns_error() {
    let mut cfg = config();
    cfg.limits.max_pending_requests = NonZeroU32::new(2).unwrap();
    let mut ns = ContextNamespace::new(cfg).expect("valid config");
    append_records(&mut ns, &["a", "b", "c", "d", "e"]);
    let view = ns.render().expect("render");

    // Prepare two requests (filling the limit).
    for i in 0..2 {
        let selection = RecordSelection {
            render_id: view.render_id,
            start_index: i,
            end_index: i + 1,
        };
        ns.prepare_record_summary(selection, make_label(&format!("s{i}")))
            .expect("prepare");
    }

    // Third should fail.
    let selection = RecordSelection {
        render_id: view.render_id,
        start_index: 2,
        end_index: 3,
    };
    let result = ns.prepare_record_summary(selection, make_label("s3"));
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Version increments on commit
// ---------------------------------------------------------------------------

#[test]
fn commit_record_summary_increments_version() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["a", "b", "c", "d"]);
    let view = ns.render().expect("render");
    let version_before = ns.version();

    let selection = RecordSelection {
        render_id: view.render_id,
        start_index: 1,
        end_index: 3,
    };
    let prepared = ns
        .prepare_record_summary(selection, make_label("batch"))
        .expect("prepare");
    let result = SummaryResult {
        summary: "Summary".to_string(),
        gloss: "gloss".to_string(),
    };
    ns.commit_summary(prepared, result).expect("commit");

    assert_eq!(ns.version(), version_before + 1);
}

// ---------------------------------------------------------------------------
// Events: prepare and commit emit events
// ---------------------------------------------------------------------------

#[test]
fn prepare_and_commit_emit_events() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["a", "b", "c"]);
    let view = ns.render().expect("render");

    // Drain events from append/render first.
    let _ = ns.drain_events(100);

    let selection = RecordSelection {
        render_id: view.render_id,
        start_index: 0,
        end_index: 2,
    };
    let prepared = ns
        .prepare_record_summary(selection, make_label("batch"))
        .expect("prepare");
    let result = SummaryResult {
        summary: "S".to_string(),
        gloss: "G".to_string(),
    };
    ns.commit_summary(prepared, result).expect("commit");

    let events = ns.drain_events(100);
    // Should have at least SummaryPrepared and SummaryCommitted.
    assert!(
        events
            .iter()
            .any(|e| matches!(e, context_namespace::NamespaceEvent::SummaryPrepared { .. }))
    );
    assert!(events.iter().any(|e| matches!(
        e,
        context_namespace::NamespaceEvent::SummaryCommitted { .. }
    )));
}

// ===========================================================================
// Text-slice summary transaction tests
// ===========================================================================

// ---------------------------------------------------------------------------
// Prepare: valid text slice by record ID
// ---------------------------------------------------------------------------

#[test]
fn prepare_text_summary_by_record_id_returns_prepared() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    let receipt = ns
        .append(make_record("hello world this is a test"))
        .expect("append");
    ns.render().expect("render");

    let selection = TextSelection {
        record: RecordLocator::Id(receipt.record_id),
        start_char: 6,
        end_char: 11,
    };
    let prepared = ns
        .prepare_text_summary(selection, make_label("slice1"))
        .expect("prepare");
    assert_eq!(prepared.input.source, "world");
}

// ---------------------------------------------------------------------------
// Prepare: valid text slice by visible index
// ---------------------------------------------------------------------------

#[test]
fn prepare_text_summary_by_visible_index_returns_prepared() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    ns.append(make_record("alpha")).expect("append");
    ns.append(make_record("beta gamma delta")).expect("append");
    ns.append(make_record("epsilon")).expect("append");
    let view = ns.render().expect("render");

    let selection = TextSelection {
        record: RecordLocator::Visible {
            render_id: view.render_id,
            index: 1,
        },
        start_char: 5,
        end_char: 10,
    };
    let prepared = ns
        .prepare_text_summary(selection, make_label("slice1"))
        .expect("prepare");
    assert_eq!(prepared.input.source, "gamma");
}

// ---------------------------------------------------------------------------
// Prepare: invalid char range (start >= end)
// ---------------------------------------------------------------------------

#[test]
fn prepare_text_summary_start_ge_end_returns_error() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    let receipt = ns.append(make_record("hello world")).expect("append");
    ns.render().expect("render");

    let selection = TextSelection {
        record: RecordLocator::Id(receipt.record_id),
        start_char: 5,
        end_char: 5,
    };
    let result = ns.prepare_text_summary(selection, make_label("slice1"));
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Prepare: end char out of bounds
// ---------------------------------------------------------------------------

#[test]
fn prepare_text_summary_end_out_of_bounds_returns_error() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    let receipt = ns.append(make_record("hello")).expect("append");
    ns.render().expect("render");

    let selection = TextSelection {
        record: RecordLocator::Id(receipt.record_id),
        start_char: 0,
        end_char: 100,
    };
    let result = ns.prepare_text_summary(selection, make_label("slice1"));
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Prepare: record not found
// ---------------------------------------------------------------------------

#[test]
fn prepare_text_summary_record_not_found_returns_error() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    ns.append(make_record("hello")).expect("append");
    ns.render().expect("render");

    let bogus_id = context_namespace::RecordId::new(uuid::Uuid::nil());
    let selection = TextSelection {
        record: RecordLocator::Id(bogus_id),
        start_char: 0,
        end_char: 3,
    };
    let result = ns.prepare_text_summary(selection, make_label("slice1"));
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Prepare: stale render ID for visible locator
// ---------------------------------------------------------------------------

#[test]
fn prepare_text_summary_stale_render_id_returns_error() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    ns.append(make_record("alpha")).expect("append");
    ns.append(make_record("beta")).expect("append");
    let view = ns.render().expect("render");

    // Mutate after render to stale the render.
    ns.append(make_record("gamma")).expect("append");

    let selection = TextSelection {
        record: RecordLocator::Visible {
            render_id: view.render_id,
            index: 0,
        },
        start_char: 0,
        end_char: 2,
    };
    let result = ns.prepare_text_summary(selection, make_label("slice1"));
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Prepare: Unicode boundary validation
// ---------------------------------------------------------------------------

#[test]
fn prepare_text_summary_unicode_boundaries_extract_correct_text() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    // "héllo wörld" — é is one scalar value (U+00E9), ö is one (U+00F6).
    let receipt = ns.append(make_record("héllo wörld")).expect("append");
    ns.render().expect("render");

    // Slice "wörld" (chars 6..11).
    let selection = TextSelection {
        record: RecordLocator::Id(receipt.record_id),
        start_char: 6,
        end_char: 11,
    };
    let prepared = ns
        .prepare_text_summary(selection, make_label("unicode"))
        .expect("prepare");
    assert_eq!(prepared.input.source, "wörld");
}

#[test]
fn prepare_text_summary_multibyte_boundary_at_start() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    // "日本語テスト" — 6 chars, all multi-byte.
    let receipt = ns.append(make_record("日本語テスト")).expect("append");
    ns.render().expect("render");

    // Slice "語テ" (chars 2..4).
    let selection = TextSelection {
        record: RecordLocator::Id(receipt.record_id),
        start_char: 2,
        end_char: 4,
    };
    let prepared = ns
        .prepare_text_summary(selection, make_label("jp"))
        .expect("prepare");
    assert_eq!(prepared.input.source, "語テ");
}

// ---------------------------------------------------------------------------
// Commit: valid text slice → creates node, replaces range in record
// ---------------------------------------------------------------------------

#[test]
fn commit_text_summary_valid_creates_node_and_edits_record() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    let receipt = ns
        .append(make_record("hello world this is a test"))
        .expect("append");
    ns.render().expect("render");

    let selection = TextSelection {
        record: RecordLocator::Id(receipt.record_id),
        start_char: 6,
        end_char: 11,
    };
    let prepared = ns
        .prepare_text_summary(selection, make_label("slice1"))
        .expect("prepare");

    let result = SummaryResult {
        summary: "The world".to_string(),
        gloss: "world summary".to_string(),
    };
    let commit_receipt = ns.commit_summary(prepared, result).expect("commit");

    // One node created.
    assert_eq!(ns.stats().nodes, 1);
    // Record count unchanged (text-slice edits in place).
    assert_eq!(ns.stats().records, 1);

    // Node resolvable by path.
    let path = NamespacePath::parse("/slice1", &DefensiveLimits::portfolio_defaults())
        .expect("valid path");
    assert_eq!(
        ns.resolve_path(&path).expect("resolve"),
        commit_receipt.new_node_id
    );
}

// ---------------------------------------------------------------------------
// Commit: text slice stale version → error, state unchanged
// ---------------------------------------------------------------------------

#[test]
fn commit_text_summary_stale_version_returns_error() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    let receipt = ns.append(make_record("hello world test")).expect("append");
    ns.render().expect("render");

    let selection = TextSelection {
        record: RecordLocator::Id(receipt.record_id),
        start_char: 0,
        end_char: 5,
    };
    let prepared = ns
        .prepare_text_summary(selection, make_label("slice1"))
        .expect("prepare");

    // Mutate after prepare.
    ns.append(make_record("new record")).expect("append");

    let result = SummaryResult {
        summary: "S".to_string(),
        gloss: "G".to_string(),
    };
    let err = ns.commit_summary(prepared, result);
    assert!(err.is_err());
    assert_eq!(ns.stats().nodes, 0);
}

// ---------------------------------------------------------------------------
// Commit: text slice result too long → error
// ---------------------------------------------------------------------------

#[test]
fn commit_text_summary_result_too_long_returns_error() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    let receipt = ns.append(make_record("hello world")).expect("append");
    ns.render().expect("render");

    let selection = TextSelection {
        record: RecordLocator::Id(receipt.record_id),
        start_char: 0,
        end_char: 5,
    };
    let prepared = ns
        .prepare_text_summary(selection, make_label("slice1"))
        .expect("prepare");

    let too_long = "x".repeat(1201);
    let result = SummaryResult {
        summary: too_long,
        gloss: "ok".to_string(),
    };
    let err = ns.commit_summary(prepared, result);
    assert!(err.is_err());
}

// ---------------------------------------------------------------------------
// Cancel: text slice pending removed
// ---------------------------------------------------------------------------

#[test]
fn cancel_text_summary_removes_pending() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    let receipt = ns.append(make_record("hello world")).expect("append");
    ns.render().expect("render");

    let selection = TextSelection {
        record: RecordLocator::Id(receipt.record_id),
        start_char: 0,
        end_char: 5,
    };
    let prepared = ns
        .prepare_text_summary(selection, make_label("slice1"))
        .expect("prepare");
    let request_id = prepared.request_id;

    ns.cancel_summary(request_id).expect("cancel");
    assert_eq!(ns.stats().nodes, 0);
    assert!(ns.cancel_summary(request_id).is_err());
}
