use context_namespace::{
    ContentHash, ContextNamespace, DefensiveLimits, MaintenanceDecision, MaintenanceInput,
    NamespaceConfig, NamespaceError, NamespacePath, NewRecord, NodeLabel, RecordTokenEstimate,
    SummaryResult,
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

fn append_records(ns: &mut ContextNamespace, contents: &[&str]) {
    for content in contents {
        ns.append(make_record(content)).expect("append");
    }
}

fn make_label(s: &str) -> NodeLabel {
    NodeLabel::new(s, &DefensiveLimits::portfolio_defaults()).expect("valid label")
}

fn parse_path(s: &str) -> NamespacePath {
    NamespacePath::parse(s, &DefensiveLimits::portfolio_defaults()).expect("valid path")
}

/// Builds a `MaintenanceInput` from a preview, assigning each unit the
/// specified tokens-per-record.
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
// preview_render tests
// ===========================================================================

#[test]
fn preview_render_returns_namespace_version() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["alpha", "beta"]);
    let preview = ns.preview_render().expect("preview");
    assert!(preview.namespace_version > 0);
}

#[test]
fn preview_render_produces_maintenance_unit_per_record() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["alpha", "beta", "gamma"]);
    let preview = ns.preview_render().expect("preview");
    assert_eq!(preview.maintenance_units.len(), 3);
}

#[test]
fn preview_render_units_contain_record_id_and_digest() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["alpha"]);
    let preview = ns.preview_render().expect("preview");
    let unit = &preview.maintenance_units[0];
    assert!(!unit.json_fragment.is_empty());
    assert_eq!(unit.render_digest.as_bytes().len(), 32);
}

#[test]
fn preview_render_does_not_decrement_ttl() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["alpha", "beta"]);
    let view = ns.render().expect("render");
    let label = make_label("sum");
    let prepared = ns
        .prepare_record_summary(
            context_namespace::RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 1,
            },
            label,
        )
        .expect("prepare");
    ns.commit_summary(
        prepared,
        SummaryResult {
            summary: "s".to_string(),
            gloss: "g".to_string(),
        },
    )
    .expect("commit");

    let node_id = ns.list_nodes()[0].node_id;
    let path = ns.current_path(node_id).expect("path");
    ns.expand(&path, 2).expect("expand");

    // Preview should not decrement TTL.
    let _preview = ns.preview_render().expect("preview");

    // A real render should still see TTL=2 (preview did not consume it).
    let _receipt = ns.render().expect("render");
    assert!(
        ns.list_nodes()
            .iter()
            .any(|n| n.node_id == node_id && n.expanded)
    );
}

// ===========================================================================
// plan_maintenance: trigger logic
// ===========================================================================

#[test]
fn plan_maintenance_below_trigger_returns_no_action() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["a", "b", "c"]);
    let preview = ns.preview_render().expect("preview");
    let input = input_from_preview(&preview, 1_000, 100);

    let decision = ns.plan_maintenance(input).expect("plan");
    assert!(matches!(decision, MaintenanceDecision::NoAction { .. }));
}

#[test]
fn plan_maintenance_above_trigger_returns_summarize() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]);
    let preview = ns.preview_render().expect("preview");
    let input = input_from_preview(&preview, 30_000, 3_000);

    let decision = ns.plan_maintenance(input).expect("plan");
    assert!(matches!(decision, MaintenanceDecision::Summarize(_)));
}

#[test]
fn plan_maintenance_exactly_at_trigger_returns_summarize() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]);
    let preview = ns.preview_render().expect("preview");
    let input = input_from_preview(&preview, 24_000, 2_400);

    let decision = ns.plan_maintenance(input).expect("plan");
    assert!(matches!(decision, MaintenanceDecision::Summarize(_)));
}

// ===========================================================================
// plan_maintenance: record selection
// ===========================================================================

#[test]
fn plan_maintenance_preserves_prefix_records() {
    let mut config = config();
    config.policy.preserve_prefix_records = 1;
    config.policy.preserve_recent_records = 2;
    let mut ns = ContextNamespace::new(config).expect("valid config");
    append_records(&mut ns, &["p0", "p1", "p2", "p3", "p4", "p5", "p6"]);
    let preview = ns.preview_render().expect("preview");
    let input = input_from_preview(&preview, 30_000, 5_000);

    let decision = ns.plan_maintenance(input).expect("plan");
    let prepared = match decision {
        MaintenanceDecision::Summarize(p) => p,
        MaintenanceDecision::NoAction { .. } => panic!("expected Summarize"),
    };
    let src = &prepared.input.source;
    assert!(src.contains("p1"));
    assert!(src.contains("p4"));
    assert!(!src.contains("p0"));
    assert!(!src.contains("p5"));
    assert!(!src.contains("p6"));
}

#[test]
fn plan_maintenance_preserves_recent_records() {
    let mut config = config();
    config.policy.preserve_prefix_records = 1;
    config.policy.preserve_recent_records = 3;
    let mut ns = ContextNamespace::new(config).expect("valid config");
    append_records(&mut ns, &["p0", "p1", "p2", "p3", "p4", "p5", "p6", "p7"]);
    let preview = ns.preview_render().expect("preview");
    let input = input_from_preview(&preview, 30_000, 4_000);

    let decision = ns.plan_maintenance(input).expect("plan");
    let prepared = match decision {
        MaintenanceDecision::Summarize(p) => p,
        MaintenanceDecision::NoAction { .. } => panic!("expected Summarize"),
    };
    let src = &prepared.input.source;
    assert!(src.contains("p1"));
    assert!(src.contains("p4"));
    assert!(!src.contains("p0"));
    assert!(!src.contains("p5"));
    assert!(!src.contains("p6"));
    assert!(!src.contains("p7"));
}

#[test]
fn plan_maintenance_no_eligible_range_returns_no_action() {
    let mut config = config();
    config.policy.preserve_prefix_records = 5;
    config.policy.preserve_recent_records = 5;
    let mut ns = ContextNamespace::new(config).expect("valid config");
    append_records(&mut ns, &["a", "b", "c", "d", "e", "f", "g", "h"]);
    let preview = ns.preview_render().expect("preview");
    let input = input_from_preview(&preview, 30_000, 4_000);

    let decision = ns.plan_maintenance(input).expect("plan");
    assert!(matches!(decision, MaintenanceDecision::NoAction { .. }));
}

// ===========================================================================
// plan_maintenance: estimate validation
// ===========================================================================

#[test]
fn plan_maintenance_rejects_mismatched_record_count() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["a", "b", "c"]);
    let preview = ns.preview_render().expect("preview");
    let records = preview
        .maintenance_units
        .iter()
        .take(2)
        .map(|unit| RecordTokenEstimate {
            record_id: unit.record_id,
            render_digest: unit.render_digest.clone(),
            estimated_tokens: 10_000,
        })
        .collect();
    let input = MaintenanceInput {
        estimated_next_tokens: 30_000,
        records,
    };

    let result = ns.plan_maintenance(input);
    assert!(result.is_err());
}

#[test]
fn plan_maintenance_rejects_stale_digest() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["a", "b", "c"]);
    let preview = ns.preview_render().expect("preview");
    let mut records: Vec<_> = preview
        .maintenance_units
        .iter()
        .map(|unit| RecordTokenEstimate {
            record_id: unit.record_id,
            render_digest: unit.render_digest.clone(),
            estimated_tokens: 10_000,
        })
        .collect();
    records[0].render_digest = ContentHash::of(b"stale");
    let input = MaintenanceInput {
        estimated_next_tokens: 30_000,
        records,
    };

    let result = ns.plan_maintenance(input);
    assert!(result.is_err());
}

#[test]
fn plan_maintenance_rejects_extra_estimates() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["a", "b"]);
    let preview = ns.preview_render().expect("preview");
    let mut records: Vec<_> = preview
        .maintenance_units
        .iter()
        .map(|unit| RecordTokenEstimate {
            record_id: unit.record_id,
            render_digest: unit.render_digest.clone(),
            estimated_tokens: 10_000,
        })
        .collect();
    records.push(RecordTokenEstimate {
        record_id: records[0].record_id,
        render_digest: records[0].render_digest.clone(),
        estimated_tokens: 10_000,
    });
    let input = MaintenanceInput {
        estimated_next_tokens: 30_000,
        records,
    };

    let result = ns.plan_maintenance(input);
    assert!(result.is_err());
}

#[test]
fn plan_maintenance_rejects_duplicate_estimates() {
    let mut config = config();
    config.policy.preserve_prefix_records = 1;
    config.policy.preserve_recent_records = 1;
    config.policy.target_tokens = 5;
    let mut ns = ContextNamespace::new(config).expect("valid config");
    append_records(&mut ns, &["a", "b", "c", "d"]);
    let preview = ns.preview_render().expect("preview");
    let mut input = input_from_preview(&preview, 30_000, 5);

    // Replace b's estimate with a second copy of c's estimate.
    input.records[1] = input.records[2].clone();

    let err = ns
        .plan_maintenance(input)
        .expect_err("duplicate estimate must be rejected");
    assert!(
        matches!(&err, NamespaceError::PolicyViolation(msg) if msg.contains("duplicate estimate")),
        "unexpected error: {err}"
    );
}

#[test]
fn plan_maintenance_accepts_zero_token_estimates() {
    let mut config = config();
    config.policy.preserve_prefix_records = 1;
    config.policy.preserve_recent_records = 1;
    config.policy.target_tokens = 5;
    let mut ns = ContextNamespace::new(config).expect("valid config");
    append_records(&mut ns, &["a", "b", "c", "d"]);
    let preview = ns.preview_render().expect("preview");

    // A zero estimate is present, not missing.
    let input = input_from_preview(&preview, 30_000, 0);

    let decision = ns
        .plan_maintenance(input)
        .expect("zero-token estimates are valid");
    assert!(matches!(decision, MaintenanceDecision::Summarize(_)));
}

// ===========================================================================
// plan_maintenance: prepared summary is committable
// ===========================================================================

#[test]
fn plan_maintenance_prepared_summary_commits_successfully() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]);
    let preview = ns.preview_render().expect("preview");
    let input = input_from_preview(&preview, 30_000, 3_000);

    let decision = ns.plan_maintenance(input).expect("plan");
    let prepared = match decision {
        MaintenanceDecision::Summarize(p) => p,
        MaintenanceDecision::NoAction { .. } => panic!("expected Summarize"),
    };

    let receipt = ns
        .commit_summary(
            prepared,
            SummaryResult {
                summary: "summarized batch".to_string(),
                gloss: "batch gloss".to_string(),
            },
        )
        .expect("commit");
    assert!(ns.validate().is_ok());
    assert_eq!(ns.list_nodes().len(), 1);
    assert!(!receipt.replaced_record_ids.is_empty());
}

#[test]
fn plan_maintenance_no_action_stats_reflect_current_state() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["a", "b"]);
    let preview = ns.preview_render().expect("preview");
    let input = input_from_preview(&preview, 1_000, 100);

    let decision = ns.plan_maintenance(input).expect("plan");
    if let MaintenanceDecision::NoAction { stats } = decision {
        assert_eq!(stats.records, 2);
        assert_eq!(stats.nodes, 0);
    } else {
        panic!("expected NoAction");
    }
}

// ===========================================================================
// plan_maintenance: expansion overlay safety
// ===========================================================================

/// Builds a namespace with a `/protected` summary node whose expansion has a
/// single render of TTL left, plus a policy that leaves a middle range
/// eligible for maintenance.
fn ns_with_short_lived_expansion() -> (ContextNamespace, context_namespace::NodeId) {
    let mut config = config();
    config.policy.preserve_prefix_records = 1;
    config.policy.preserve_recent_records = 1;
    config.policy.trigger_tokens = 1_000;
    config.policy.target_tokens = 100;
    let mut ns = ContextNamespace::new(config).expect("valid config");
    append_records(&mut ns, &["p0", "p1", "p2", "p3"]);

    let view = ns.render().expect("render");
    let prepared = ns
        .prepare_record_summary(
            context_namespace::RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 1,
            },
            make_label("protected"),
        )
        .expect("prepare");
    ns.commit_summary(
        prepared,
        SummaryResult {
            summary: "protected summary".to_string(),
            gloss: "protected gloss".to_string(),
        },
    )
    .expect("commit");

    let node_id = ns
        .list_nodes()
        .into_iter()
        .find(|n| n.label == "protected")
        .expect("protected node")
        .node_id;
    ns.expand(&parse_path("/protected"), 1).expect("expand");
    (ns, node_id)
}

#[test]
fn plan_maintenance_preserves_expansion_ttl() {
    let (mut ns, node_id) = ns_with_short_lived_expansion();
    let preview = ns.preview_render().expect("preview");
    let input = input_from_preview(&preview, 50_000, 5_000);

    let decision = ns.plan_maintenance(input).expect("plan");
    assert!(matches!(decision, MaintenanceDecision::Summarize(_)));

    // The expansion's last render belongs to the caller, not to planning.
    assert!(ns.is_expanded(node_id), "planning consumed the last TTL");
    let view = ns.render().expect("render");
    assert!(view.json.contains("p0"), "expanded node renders its source");
}

#[test]
fn plan_maintenance_selected_range_excludes_preserved_records() {
    let (mut ns, _node_id) = ns_with_short_lived_expansion();
    let preview = ns.preview_render().expect("preview");
    let input = input_from_preview(&preview, 50_000, 5_000);

    let decision = ns.plan_maintenance(input).expect("plan");
    let prepared = match decision {
        MaintenanceDecision::Summarize(p) => p,
        MaintenanceDecision::NoAction { .. } => panic!("expected Summarize"),
    };
    let source = &prepared.input.source;
    assert!(source.contains("p1"));
    assert!(!source.contains("p2"));
    assert!(!source.contains("p3"));
}
