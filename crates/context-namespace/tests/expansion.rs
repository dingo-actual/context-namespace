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

/// Creates a namespace with one summary node at `/batch` covering records
/// 1..3. Returns the namespace and the new node's ID.
fn ns_with_summary() -> (ContextNamespace, context_namespace::NodeId) {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    for content in &["alpha", "beta", "gamma", "delta"] {
        ns.append(make_record(content)).expect("append");
    }
    let view = ns.render().expect("render");
    let sel = RecordSelection {
        render_id: view.render_id,
        start_index: 1,
        end_index: 3,
    };
    let prepared = ns
        .prepare_record_summary(sel, make_label("batch"))
        .expect("prepare");
    let result = SummaryResult {
        summary: "Beta and gamma summarized".to_string(),
        gloss: "batch".to_string(),
    };
    let receipt = ns.commit_summary(prepared, result).expect("commit");
    (ns, receipt.new_node_id)
}

// ---------------------------------------------------------------------------
// Expand: valid activation
// ---------------------------------------------------------------------------

#[test]
fn expand_valid_path_activates_expansion() {
    let (mut ns, node_id) = ns_with_summary();
    let path =
        NamespacePath::parse("/batch", &DefensiveLimits::portfolio_defaults()).expect("valid path");

    let receipt = ns.expand(&path, 2).expect("expand");
    assert_eq!(receipt.node_id, node_id);
    assert_eq!(receipt.ttl_renders, 2);
    assert!(ns.is_expanded(node_id));
}

// ---------------------------------------------------------------------------
// Expand: unknown path → error
// ---------------------------------------------------------------------------

#[test]
fn expand_unknown_path_returns_error() {
    let (mut ns, _) = ns_with_summary();
    let path = NamespacePath::parse("/nonexistent", &DefensiveLimits::portfolio_defaults())
        .expect("valid path");
    let result = ns.expand(&path, 2);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Expand: TTL exceeds max → error
// ---------------------------------------------------------------------------

#[test]
fn expand_ttl_exceeds_max_returns_error() {
    let (mut ns, _) = ns_with_summary();
    let path =
        NamespacePath::parse("/batch", &DefensiveLimits::portfolio_defaults()).expect("valid path");
    // Default max_expansion_ttl is 4.
    let result = ns.expand(&path, 5);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Expand: TTL zero → error
// ---------------------------------------------------------------------------

#[test]
fn expand_ttl_zero_returns_error() {
    let (mut ns, _) = ns_with_summary();
    let path =
        NamespacePath::parse("/batch", &DefensiveLimits::portfolio_defaults()).expect("valid path");
    let result = ns.expand(&path, 0);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Re-expansion: max(current, requested), never shortens
// ---------------------------------------------------------------------------

#[test]
fn re_expansion_never_shortens_ttl() {
    let (mut ns, node_id) = ns_with_summary();
    let path =
        NamespacePath::parse("/batch", &DefensiveLimits::portfolio_defaults()).expect("valid path");

    ns.expand(&path, 3).expect("expand");
    // Re-expand with lower TTL — should keep 3, not shorten to 1.
    let receipt = ns.expand(&path, 1).expect("re-expand");
    assert_eq!(receipt.ttl_renders, 3);
    assert!(ns.is_expanded(node_id));
}

// ---------------------------------------------------------------------------
// Re-expansion: higher TTL takes effect
// ---------------------------------------------------------------------------

#[test]
fn re_expansion_higher_ttl_takes_effect() {
    let (mut ns, _) = ns_with_summary();
    let path =
        NamespacePath::parse("/batch", &DefensiveLimits::portfolio_defaults()).expect("valid path");

    ns.expand(&path, 1).expect("expand");
    let receipt = ns.expand(&path, 3).expect("re-expand");
    assert_eq!(receipt.ttl_renders, 3);
}

// ---------------------------------------------------------------------------
// Render: TTL decrements on successful render
// ---------------------------------------------------------------------------

#[test]
fn render_decrements_expansion_ttl() {
    let (mut ns, node_id) = ns_with_summary();
    let path =
        NamespacePath::parse("/batch", &DefensiveLimits::portfolio_defaults()).expect("valid path");

    ns.expand(&path, 2).expect("expand");
    assert!(ns.is_expanded(node_id));

    // First render — TTL becomes 1.
    ns.render().expect("render");
    assert!(ns.is_expanded(node_id));

    // Second render — TTL becomes 0, expansion expires.
    ns.render().expect("render");
    assert!(!ns.is_expanded(node_id));
}

// ---------------------------------------------------------------------------
// Render: expansion overlay inlines the node's stored source text
// ---------------------------------------------------------------------------

#[test]
fn render_with_expansion_inlines_stored_source_text() {
    let (mut ns, _node_id) = ns_with_summary();
    let path =
        NamespacePath::parse("/batch", &DefensiveLimits::portfolio_defaults()).expect("valid path");

    // Render without expansion — the collapsed marker shows the node's
    // path and summary text.
    let collapsed = ns.render().expect("render");
    let collapsed_json = collapsed.json.clone();
    assert!(collapsed_json.contains("[summary(/batch): Beta and gamma summarized]"));

    // Expand and render — the node's stored source (records "beta" and
    // "gamma" joined by a newline) is inlined in place of the marker, with
    // no `[expanded: ...]` wrapper.
    ns.expand(&path, 2).expect("expand");
    let expanded = ns.render().expect("render");
    let expanded_json = expanded.json;
    assert!(expanded_json.contains(r"beta\ngamma"));
    assert!(!expanded_json.contains("[summary("));
    assert!(!expanded_json.contains("[expanded:"));
}

#[test]
fn expanded_text_slice_node_inlines_source_segments_exactly_once() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    ns.append(make_record("alpha bravo charlie"))
        .expect("append");

    // First slice: summarize "bravo" (chars 6..11) → /slice-1.
    let view = ns.render().expect("render");
    let prepared1 = ns
        .prepare_text_summary(
            TextSelection {
                record: RecordLocator::Visible {
                    render_id: view.render_id,
                    index: 0,
                },
                start_char: 6,
                end_char: 11,
            },
            make_label("slice-1"),
        )
        .expect("prepare 1");
    ns.commit_summary(
        prepared1,
        SummaryResult {
            summary: "Bravo".to_string(),
            gloss: "bravo".to_string(),
        },
    )
    .expect("commit 1");

    // Second slice over the whole remaining render field: it covers both
    // text segments plus the slice-1 ref, so slice-1 nests under slice-2.
    let view2 = ns.render().expect("render");
    let prepared2 = ns
        .prepare_text_summary(
            TextSelection {
                record: RecordLocator::Visible {
                    render_id: view2.render_id,
                    index: 0,
                },
                start_char: 0,
                end_char: 14,
            },
            make_label("slice-2"),
        )
        .expect("prepare 2");
    ns.commit_summary(
        prepared2,
        SummaryResult {
            summary: "Everything".to_string(),
            gloss: "everything".to_string(),
        },
    )
    .expect("commit 2");

    // Expand /slice-2: its stored source must inline exactly once per
    // segment, with the nested slice-1 ref rendered as a collapsed
    // marker leading with its current path and summary text.
    let path = NamespacePath::parse("/slice-2", &DefensiveLimits::portfolio_defaults())
        .expect("valid path");
    ns.expand(&path, 2).expect("expand");
    let view3 = ns.render().expect("render");
    let json = view3.json;
    assert!(
        json.contains("alpha [summary(/slice-2/slice-1): Bravo] charlie"),
        "nested collapsed ref should render as its path-prefixed summary marker"
    );
    assert!(
        !json.contains("alpha  charlie"),
        "stored source must not duplicate the extracted range per text segment"
    );
    assert!(!json.contains("[expanded:"));

    // Now expand the nested slice-1 as well: its stored source inlines
    // recursively, restoring the original record text in full.
    let path1 = NamespacePath::parse("/slice-2/slice-1", &DefensiveLimits::portfolio_defaults())
        .expect("valid nested path");
    ns.expand(&path1, 1).expect("expand nested");
    let view4 = ns.render().expect("render nested expanded");
    assert!(view4.json.contains("alpha bravo charlie"));
    assert!(!view4.json.contains("[summary("));
    ns.validate().expect("invariants after nested expansion");
}

#[test]
fn disjoint_text_slices_round_trip_to_original_text() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    ns.append(make_record("abcdef")).expect("append");

    // Slice "cd" out, so the record render becomes text + ref + text.
    let view = ns.render().expect("render");
    let prepared = ns
        .prepare_text_summary(
            TextSelection {
                record: RecordLocator::Visible {
                    render_id: view.render_id,
                    index: 0,
                },
                start_char: 2,
                end_char: 4,
            },
            make_label("cd"),
        )
        .expect("prepare cd");
    ns.commit_summary(
        prepared,
        SummaryResult {
            summary: "C D".to_string(),
            gloss: "cd".to_string(),
        },
    )
    .expect("commit cd");

    // Slice "a" out of the leading text segment.
    let view = ns.render().expect("render");
    let prepared = ns
        .prepare_text_summary(
            TextSelection {
                record: RecordLocator::Visible {
                    render_id: view.render_id,
                    index: 0,
                },
                start_char: 0,
                end_char: 1,
            },
            make_label("a"),
        )
        .expect("prepare a");
    ns.commit_summary(
        prepared,
        SummaryResult {
            summary: "A".to_string(),
            gloss: "a".to_string(),
        },
    )
    .expect("commit a");

    for label in ["/a", "/cd"] {
        let path = NamespacePath::parse(label, &DefensiveLimits::portfolio_defaults())
            .expect("valid path");
        ns.expand(&path, 1).expect("expand");
    }

    let view = ns.render().expect("render");
    assert!(
        view.json.contains(r#""content":"abcdef""#),
        "expanded slices must reconstruct the source text once: {}",
        view.json
    );
    ns.validate().expect("invariants after expansion");
}

#[test]
fn text_slice_starting_on_a_zero_length_ref_nests_it() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    ns.append(make_record("abcdef")).expect("append");

    // Slice "cd" out, so the record render becomes "ab" + ref + "ef".
    let view = ns.render().expect("render");
    let prepared = ns
        .prepare_text_summary(
            TextSelection {
                record: RecordLocator::Visible {
                    render_id: view.render_id,
                    index: 0,
                },
                start_char: 2,
                end_char: 4,
            },
            make_label("cd"),
        )
        .expect("prepare cd");
    ns.commit_summary(
        prepared,
        SummaryResult {
            summary: "C D".to_string(),
            gloss: "cd".to_string(),
        },
    )
    .expect("commit cd");

    // Slice "e": the range starts exactly on the /cd ref, which has zero
    // char width, so /cd must become a child of /e.
    let view = ns.render().expect("render");
    let prepared = ns
        .prepare_text_summary(
            TextSelection {
                record: RecordLocator::Visible {
                    render_id: view.render_id,
                    index: 0,
                },
                start_char: 2,
                end_char: 3,
            },
            make_label("e"),
        )
        .expect("prepare e");
    ns.commit_summary(
        prepared,
        SummaryResult {
            summary: "E".to_string(),
            gloss: "e".to_string(),
        },
    )
    .expect("commit e");

    let view = ns.render().expect("render");
    assert!(
        view.json.contains("[summary(/e): E; gloss(/e/cd): cd]"),
        "a ref at the slice start must nest under the slice: {}",
        view.json
    );
    assert!(!view.json.contains("[summary(/cd)"));
    ns.validate().expect("invariants after boundary ref slice");
}

// ---------------------------------------------------------------------------
// Render: expansion expires after TTL renders
// ---------------------------------------------------------------------------

#[test]
fn expansion_expires_after_ttl_renders() {
    let (mut ns, node_id) = ns_with_summary();
    let path =
        NamespacePath::parse("/batch", &DefensiveLimits::portfolio_defaults()).expect("valid path");

    ns.expand(&path, 1).expect("expand");
    assert!(ns.is_expanded(node_id));

    // One render consumes the TTL.
    ns.render().expect("render");
    assert!(!ns.is_expanded(node_id));

    // Next render should show collapsed content again.
    let r = ns.render().expect("render");
    assert!(
        r.json
            .contains("[summary(/batch): Beta and gamma summarized]")
    );
}

// ---------------------------------------------------------------------------
// Expand: max active expansions limit
// ---------------------------------------------------------------------------

#[test]
fn expand_max_active_expansions_exceeded() {
    let mut cfg = config();
    cfg.limits.max_active_expansions = NonZeroU32::new(1).unwrap();
    let mut ns = ContextNamespace::new(cfg).expect("valid config");

    // Create two summary nodes.
    for content in &["a", "b", "c", "d"] {
        ns.append(make_record(content)).expect("append");
    }

    // First summary: records 0..2 → /s1.
    let view1 = ns.render().expect("render");
    let sel1 = RecordSelection {
        render_id: view1.render_id,
        start_index: 0,
        end_index: 2,
    };
    let prepared1 = ns
        .prepare_record_summary(sel1, make_label("s1"))
        .expect("prepare");
    ns.commit_summary(
        prepared1,
        SummaryResult {
            summary: "S1".to_string(),
            gloss: "g1".to_string(),
        },
    )
    .expect("commit");

    // Second summary: records 1..3 → /s2.
    // After first commit: a, [carrier], c, d → 3 records.
    // Visible: 0=a, 1=carrier, 2=d.
    // Summarize visible 1..3 (carrier and d).
    let view2 = ns.render().expect("render");
    let sel2 = RecordSelection {
        render_id: view2.render_id,
        start_index: 1,
        end_index: 3,
    };
    let prepared2 = ns
        .prepare_record_summary(sel2, make_label("s2"))
        .expect("prepare");
    ns.commit_summary(
        prepared2,
        SummaryResult {
            summary: "S2".to_string(),
            gloss: "g2".to_string(),
        },
    )
    .expect("commit");

    // Expand first node — should succeed.
    let path1 =
        NamespacePath::parse("/s1", &DefensiveLimits::portfolio_defaults()).expect("valid path");
    ns.expand(&path1, 1).expect("expand first");

    // Expand second node — should fail (max 1 active).
    let path2 =
        NamespacePath::parse("/s2", &DefensiveLimits::portfolio_defaults()).expect("valid path");
    let result = ns.expand(&path2, 1);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Events: expansion activated and expired events
// ---------------------------------------------------------------------------

#[test]
fn expansion_emits_activated_and_expired_events() {
    let (mut ns, _node_id) = ns_with_summary();
    let path =
        NamespacePath::parse("/batch", &DefensiveLimits::portfolio_defaults()).expect("valid path");

    // Drain prior events.
    let _ = ns.drain_events(100);

    ns.expand(&path, 1).expect("expand");
    ns.render().expect("render"); // TTL → 0, expires.

    let events = ns.drain_events(100);
    assert!(events.iter().any(|e| matches!(
        e,
        context_namespace::NamespaceEvent::ExpansionActivated { .. }
    )));
    assert!(events.iter().any(|e| matches!(
        e,
        context_namespace::NamespaceEvent::ExpansionExpired { .. }
    )));
}
