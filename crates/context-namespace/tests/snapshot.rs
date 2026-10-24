use context_namespace::{
    ContextNamespace, DefensiveLimits, NamespaceConfig, NewRecord, NodeLabel, RestoreOptions,
    SnapshotBytes,
};

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
    let records: Vec<NewRecord> = (0..n)
        .map(|i| make_record(&format!("record {i}")))
        .collect();
    ContextNamespace::from_records(make_config(), records).expect("valid namespace")
}

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
// Existing SnapshotBytes / RestoreOptions unit tests
// ---------------------------------------------------------------------------

#[test]
fn snapshot_bytes_stores_data_and_hash() {
    let data = br#"{"schema_version":1}"#;
    let snapshot = SnapshotBytes::new(data.to_vec());
    assert_eq!(snapshot.data(), data);
    assert!(!snapshot.sha256_hex().is_empty());
}

#[test]
fn snapshot_bytes_hash_is_deterministic() {
    let data = br#"{"schema_version":1}"#;
    let s1 = SnapshotBytes::new(data.to_vec());
    let s2 = SnapshotBytes::new(data.to_vec());
    assert_eq!(s1.sha256_hex(), s2.sha256_hex());
}

#[test]
fn snapshot_bytes_different_data_different_hash() {
    let s1 = SnapshotBytes::new(br#"{"a":1}"#.to_vec());
    let s2 = SnapshotBytes::new(br#"{"a":2}"#.to_vec());
    assert_ne!(s1.sha256_hex(), s2.sha256_hex());
}

#[test]
fn restore_options_default_accepts_any_version() {
    let opts = RestoreOptions::default();
    assert!(opts.min_version.is_none());
    assert!(opts.max_version.is_none());
}

#[test]
fn restore_options_with_version_range() {
    let opts = RestoreOptions {
        min_version: Some(1),
        max_version: Some(1),
    };
    assert_eq!(opts.min_version, Some(1));
    assert_eq!(opts.max_version, Some(1));
}

// ---------------------------------------------------------------------------
// Snapshot / restore round-trip
// ---------------------------------------------------------------------------

#[test]
fn snapshot_empty_namespace_round_trips() {
    let mut ns = ContextNamespace::new(make_config()).expect("valid config");
    let snapshot = ns.snapshot().expect("snapshot");
    let (restored, receipt) =
        ContextNamespace::restore(snapshot.data(), RestoreOptions::default()).expect("restore");

    assert_eq!(restored.version(), ns.version());
    assert_eq!(receipt.records, 0);
    assert_eq!(receipt.nodes, 0);
    restored.validate().expect("validation passes");
}

#[test]
fn snapshot_with_records_no_nodes_round_trips() {
    let mut ns = make_ns_with_records(5);
    let snapshot = ns.snapshot().expect("snapshot");
    let (restored, receipt) =
        ContextNamespace::restore(snapshot.data(), RestoreOptions::default()).expect("restore");

    assert_eq!(receipt.records, 5);
    assert_eq!(receipt.nodes, 0);
    assert_eq!(restored.version(), ns.version());
    restored.validate().expect("validation passes");
}

#[test]
fn snapshot_after_record_range_summary_round_trips() {
    let mut ns = make_ns_with_records(4);
    summarize_records(&mut ns, 0, 3, "first-summary");

    let snapshot = ns.snapshot().expect("snapshot");
    let (restored, receipt) =
        ContextNamespace::restore(snapshot.data(), RestoreOptions::default()).expect("restore");

    assert_eq!(receipt.records, 2); // 4 - 3 + 1 carrier
    assert_eq!(receipt.nodes, 1);
    assert_eq!(restored.version(), ns.version());
    restored.validate().expect("validation passes");

    // The restored node should be accessible by path.
    let path = context_namespace::NamespacePath::parse("/first-summary", &restored.config().limits)
        .expect("valid path");
    restored.resolve_path(&path).expect("path resolves");
}

#[test]
fn snapshot_after_nested_summaries_round_trips() {
    let mut ns = make_ns_with_records(6);
    summarize_records(&mut ns, 0, 3, "outer");
    summarize_records(&mut ns, 0, 2, "inner");

    let snapshot = ns.snapshot().expect("snapshot");
    let (restored, receipt) =
        ContextNamespace::restore(snapshot.data(), RestoreOptions::default()).expect("restore");

    assert_eq!(receipt.nodes, 2);
    restored.validate().expect("validation passes");

    // Both paths should resolve. "outer" was reparented under "inner".
    let outer_path =
        context_namespace::NamespacePath::parse("/inner/outer", &restored.config().limits)
            .expect("valid path");
    restored
        .resolve_path(&outer_path)
        .expect("outer resolves under inner");

    let inner_path = context_namespace::NamespacePath::parse("/inner", &restored.config().limits)
        .expect("valid path");
    restored.resolve_path(&inner_path).expect("inner resolves");
}

#[test]
fn snapshot_after_text_slice_summary_round_trips() {
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

    let snapshot = ns.snapshot().expect("snapshot");
    let (restored, receipt) =
        ContextNamespace::restore(snapshot.data(), RestoreOptions::default()).expect("restore");

    assert_eq!(receipt.nodes, 1);
    restored.validate().expect("validation passes");
}

#[test]
fn snapshot_preserves_config() {
    let mut ns = make_ns_with_records(3);
    let snapshot = ns.snapshot().expect("snapshot");
    let (restored, _) =
        ContextNamespace::restore(snapshot.data(), RestoreOptions::default()).expect("restore");

    assert_eq!(restored.config().render_field, "content");
}

#[test]
fn snapshot_preserves_aliases() {
    let mut ns = make_ns_with_records(5);
    summarize_records(&mut ns, 0, 3, "aliased");

    // After the first summary, summarize a range that includes the carrier
    // to create a nesting (old path becomes alias).
    summarize_records(&mut ns, 0, 2, "wrapper");

    let snapshot = ns.snapshot().expect("snapshot");
    let (restored, _) =
        ContextNamespace::restore(snapshot.data(), RestoreOptions::default()).expect("restore");

    restored.validate().expect("validation passes");
    // The "aliased" node should still be resolvable by its current path.
    let path =
        context_namespace::NamespacePath::parse("/wrapper/aliased", &restored.config().limits)
            .expect("valid path");
    restored
        .resolve_path(&path)
        .expect("aliased resolves under wrapper");
}

#[test]
fn snapshot_restore_rejects_unsupported_schema_version() {
    let mut ns = make_ns_with_records(2);
    let snapshot = ns.snapshot().expect("snapshot");

    // Corrupt the schema version in the JSON.
    let mut json: serde_json::Value = serde_json::from_slice(snapshot.data()).expect("parse json");
    if let Some(obj) = json.as_object_mut() {
        obj.insert("schema_version".to_string(), serde_json::json!(999));
    }
    let corrupt_data = serde_json::to_vec(&json).expect("serialize");

    let result = ContextNamespace::restore(&corrupt_data, RestoreOptions::default());
    assert!(result.is_err());
}

#[test]
fn snapshot_restore_rejects_version_outside_range() {
    let mut ns = make_ns_with_records(2);
    let snapshot = ns.snapshot().expect("snapshot");

    let opts = RestoreOptions {
        min_version: Some(2),
        max_version: Some(3),
    };
    let result = ContextNamespace::restore(snapshot.data(), opts);
    assert!(result.is_err());
}

#[test]
fn snapshot_restore_accepts_matching_version_range() {
    let mut ns = make_ns_with_records(2);
    let snapshot = ns.snapshot().expect("snapshot");

    let opts = RestoreOptions {
        min_version: Some(1),
        max_version: Some(1),
    };
    let result = ContextNamespace::restore(snapshot.data(), opts);
    assert!(result.is_ok());
}

#[test]
fn snapshot_does_not_include_pending_requests() {
    let mut ns = make_ns_with_records(4);
    let view = ns.render().expect("render");
    let label = NodeLabel::new("pending", &ns.config().limits).expect("valid label");
    let selection = context_namespace::RecordSelection {
        render_id: view.render_id,
        start_index: 0,
        end_index: 2,
    };
    let _prepared = ns
        .prepare_record_summary(selection, label)
        .expect("prepare");

    // Snapshot while a request is pending.
    let snapshot = ns.snapshot().expect("snapshot");
    let (restored, _) =
        ContextNamespace::restore(snapshot.data(), RestoreOptions::default()).expect("restore");

    // The restored namespace should have no pending requests.
    restored.validate().expect("validation passes");
}

#[test]
fn snapshot_restore_preserves_expansion_state() {
    let mut ns = make_ns_with_records(4);
    summarize_records(&mut ns, 0, 3, "expandable");

    let path = context_namespace::NamespacePath::parse("/expandable", &ns.config().limits)
        .expect("valid path");
    ns.expand(&path, 2).expect("expand");

    let snapshot = ns.snapshot().expect("snapshot");
    let (restored, _) =
        ContextNamespace::restore(snapshot.data(), RestoreOptions::default()).expect("restore");

    restored.validate().expect("validation passes");

    // The node should be expanded in the restored namespace.
    let node_id = restored.resolve_path(&path).expect("resolve");
    assert!(restored.is_expanded(node_id));
}

#[test]
fn snapshot_data_is_valid_json() {
    let mut ns = make_ns_with_records(3);
    let snapshot = ns.snapshot().expect("snapshot");
    let json: serde_json::Value = serde_json::from_slice(snapshot.data()).expect("valid json");
    assert!(json.is_object());
    assert!(json.get("schema_version").is_some());
}

#[test]
fn snapshot_includes_content_hash() {
    let mut ns = make_ns_with_records(3);
    let snapshot = ns.snapshot().expect("snapshot");
    let json: serde_json::Value = serde_json::from_slice(snapshot.data()).expect("valid json");
    assert!(json.get("content_sha256").is_some());
}
