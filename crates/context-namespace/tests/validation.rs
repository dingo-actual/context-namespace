use context_namespace::{ContextNamespace, DefensiveLimits, NamespaceConfig, NewRecord, NodeLabel};

use indexmap::IndexMap;

fn make_config() -> NamespaceConfig {
    NamespaceConfig {
        render_field: "content".to_string(),
        ..Default::default()
    }
}

fn make_record(content: &str) -> NewRecord {
    let mut fields = IndexMap::new();
    fields.insert("content".to_string(), serde_json::json!(content));
    fields.insert("role".to_string(), serde_json::json!("user"));
    NewRecord::new(fields, "content", &DefensiveLimits::default()).expect("valid record")
}

fn make_ns_with_records(n: usize) -> ContextNamespace {
    let config = make_config();
    let records: Vec<NewRecord> = (0..n)
        .map(|i| make_record(&format!("record {i}")))
        .collect();
    ContextNamespace::from_records(config, records).expect("valid namespace")
}

/// Helper: append a record, render, prepare record summary, commit.
fn summarize_records(
    ns: &mut ContextNamespace,
    start: u32,
    end: u32,
    label: &str,
) -> context_namespace::SummaryReceipt {
    let view = ns.render().expect("render");
    let label = NodeLabel::new(label, &ns.config().limits).expect("valid label");
    let selection = context_namespace::RecordSelection {
        render_id: view.render_id,
        start_index: start,
        end_index: end,
    };
    let prepared = ns
        .prepare_record_summary(selection, label)
        .expect("prepare");
    let result = context_namespace::SummaryResult {
        summary: "Summary text".to_string(),
        gloss: "Gloss".to_string(),
    };
    ns.commit_summary(prepared, result).expect("commit")
}

// ---------------------------------------------------------------------------
// Validation on a fresh namespace
// ---------------------------------------------------------------------------

#[test]
fn validate_empty_namespace_succeeds() {
    let ns = ContextNamespace::new(make_config()).expect("valid config");
    let report = ns.validate().expect("validation passes");
    assert_eq!(report.checked_records, 0);
    assert_eq!(report.checked_nodes, 0);
    assert_eq!(report.checked_references, 0);
    assert!(report.warnings.is_empty());
}

#[test]
fn validate_namespace_with_records_no_nodes_succeeds() {
    let ns = make_ns_with_records(5);
    let report = ns.validate().expect("validation passes");
    assert_eq!(report.checked_records, 5);
    assert_eq!(report.checked_nodes, 0);
    assert_eq!(report.checked_references, 0);
    assert!(report.warnings.is_empty());
}

#[test]
fn validate_after_single_record_range_summary_succeeds() {
    let mut ns = make_ns_with_records(4);
    summarize_records(&mut ns, 0, 3, "first-summary");

    let report = ns.validate().expect("validation passes");
    assert_eq!(report.checked_records, 2); // 4 - 3 + 1 carrier
    assert_eq!(report.checked_nodes, 1);
    assert_eq!(report.checked_references, 1); // carrier -> new node
    assert!(report.warnings.is_empty());
}

#[test]
fn validate_after_nested_summary_succeeds() {
    let mut ns = make_ns_with_records(6);
    summarize_records(&mut ns, 0, 3, "outer");

    let report = ns.validate().expect("validation passes");
    assert_eq!(report.checked_nodes, 1);
    assert!(report.warnings.is_empty());

    // Nest a second summary that wraps the carrier of the first.
    summarize_records(&mut ns, 0, 2, "inner");

    let report = ns.validate().expect("validation passes");
    assert_eq!(report.checked_nodes, 2);
    assert!(report.warnings.is_empty());
}

#[test]
fn validate_after_text_slice_summary_succeeds() {
    let mut ns = make_ns_with_records(2);
    let view = ns.render().expect("render");
    let label = NodeLabel::new("text-summary", &ns.config().limits).expect("valid label");
    let selection = context_namespace::TextSelection {
        record: context_namespace::RecordLocator::Visible {
            render_id: view.render_id,
            index: 0,
        },
        start_char: 0,
        end_char: 4,
    };
    let prepared = ns.prepare_text_summary(selection, label).expect("prepare");
    let result = context_namespace::SummaryResult {
        summary: "Sum".to_string(),
        gloss: "G".to_string(),
    };
    ns.commit_summary(prepared, result).expect("commit");

    let report = ns.validate().expect("validation passes");
    assert_eq!(report.checked_nodes, 1);
    assert_eq!(report.checked_references, 1);
    assert!(report.warnings.is_empty());
}

// ---------------------------------------------------------------------------
// Validation report fields are accurate
// ---------------------------------------------------------------------------

#[test]
fn validate_report_namespace_version_matches() {
    let mut ns = make_ns_with_records(3);
    summarize_records(&mut ns, 0, 2, "summary");
    let report = ns.validate().expect("validation passes");
    assert_eq!(report.namespace_version, ns.version());
}

#[test]
fn validate_report_counts_references_correctly() {
    let mut ns = make_ns_with_records(6);
    summarize_records(&mut ns, 0, 2, "summary-a");
    // After first summary: [carrier_a, rec2, rec3, rec4, rec5]
    // carrier_a has one Ref -> node_a
    assert_eq!(ns.validate().unwrap().checked_references, 1);

    summarize_records(&mut ns, 2, 4, "summary-b");
    // After second summary: [carrier_a, carrier_b, rec5]
    // carrier_b has one Ref -> node_b
    assert_eq!(ns.validate().unwrap().checked_references, 2);
}

// ---------------------------------------------------------------------------
// Validation after expansion
// ---------------------------------------------------------------------------

#[test]
fn validate_after_expand_succeeds() {
    let mut ns = make_ns_with_records(4);
    summarize_records(&mut ns, 0, 3, "expandable");

    let path = context_namespace::NamespacePath::parse("/expandable", &ns.config().limits)
        .expect("valid path");
    ns.expand(&path, 2).expect("expand");

    let report = ns.validate().expect("validation passes");
    assert!(report.warnings.is_empty());
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn validate_with_empty_namespace_zero_counts() {
    let ns = ContextNamespace::new(make_config()).expect("valid config");
    let report = ns.validate().expect("validation passes");
    assert_eq!(report.namespace_version, 0);
}

#[test]
fn validate_after_multiple_summaries_and_expansion_succeeds() {
    let mut ns = make_ns_with_records(10);

    // Create first summary.
    summarize_records(&mut ns, 0, 3, "summary-1");

    // Expand it.
    let path = context_namespace::NamespacePath::parse("/summary-1", &ns.config().limits)
        .expect("valid path");
    ns.expand(&path, 3).expect("expand");

    // Create second summary.
    summarize_records(&mut ns, 1, 3, "summary-2");

    let report = ns.validate().expect("validation passes");
    assert_eq!(report.checked_nodes, 2);
    assert!(report.warnings.is_empty());
}
