//! End-to-end tests for the high-level `SimpleContextNamespace` API.
//!
//! These tests exercise the full lifecycle through the simplified wrapper:
//! create, append, render, summarize, expand, collapse, list, validate,
//! snapshot, restore, and capabilities — all through the easy-to-use
//! methods that hide the low-level transaction details.

use context_namespace::{
    NamespaceConfig, NodeFilter, SimpleContextNamespace, Summarizer, SummaryInput, SummaryResult,
};

use async_trait::async_trait;

fn config() -> NamespaceConfig {
    NamespaceConfig {
        render_field: "content".to_string(),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Test doubles
// ---------------------------------------------------------------------------

/// A trivial summarizer that concatenates source text and returns a
/// fixed-length gloss.
struct StubSummarizer;

#[async_trait]
impl Summarizer for StubSummarizer {
    type Error = std::convert::Infallible;

    async fn summarize(&self, input: SummaryInput) -> Result<SummaryResult, Self::Error> {
        let summary = format!("Summary of {} chars", input.source.len());
        let gloss = if input.source.len() > 100 {
            "large batch".to_string()
        } else {
            "small batch".to_string()
        };
        Ok(SummaryResult { summary, gloss })
    }
}

/// A summarizer that always fails.
struct FailingSummarizer;

/// Error type for the failing summarizer.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct StringError(String);

#[async_trait]
impl Summarizer for FailingSummarizer {
    type Error = StringError;

    async fn summarize(&self, _input: SummaryInput) -> Result<SummaryResult, Self::Error> {
        Err(StringError("model unavailable".to_string()))
    }
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

#[tokio::test]
async fn simple_ns_constructs_with_default_config() {
    let ns = SimpleContextNamespace::new(config()).expect("valid config");
    assert_eq!(ns.version(), 0);
    assert_eq!(ns.stats().records, 0);
}

#[tokio::test]
async fn simple_ns_constructs_from_simple_records() {
    let ns = SimpleContextNamespace::from_simple_records(
        config(),
        &[("user", "hello"), ("assistant", "hi there")],
    )
    .expect("construct");

    assert_eq!(ns.stats().records, 2);
}

// ---------------------------------------------------------------------------
// Append
// ---------------------------------------------------------------------------

#[tokio::test]
async fn simple_append_adds_record_and_increments_version() {
    let mut ns = SimpleContextNamespace::new(config()).expect("valid config");

    let receipt = ns.append("user", "what is 2+2?").expect("append");
    assert_eq!(ns.version(), 1);
    assert_eq!(ns.stats().records, 1);
    assert_eq!(receipt.visible_index, 0);
}

#[tokio::test]
async fn simple_append_multiple_records_assigns_consecutive_indices() {
    let mut ns = SimpleContextNamespace::new(config()).expect("valid config");

    let r0 = ns.append("user", "first").expect("append");
    let r1 = ns.append("assistant", "second").expect("append");
    let r2 = ns.append("user", "third").expect("append");

    assert_eq!(r0.visible_index, 0);
    assert_eq!(r1.visible_index, 1);
    assert_eq!(r2.visible_index, 2);
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

#[tokio::test]
async fn simple_render_returns_json_and_entry_count() {
    let mut ns = SimpleContextNamespace::new(config()).expect("valid config");
    ns.append("user", "hello").expect("append");
    ns.append("assistant", "world").expect("append");

    let view = ns.render().expect("render");

    assert_eq!(view.entries, 2);
    assert!(!view.json.is_empty());
    assert!(view.bytes > 0);
}

// ---------------------------------------------------------------------------
// Summarize records (high-level async)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn simple_summarize_records_with_commits_and_replaces_records() {
    let mut ns = SimpleContextNamespace::from_simple_records(
        config(),
        &[
            ("user", "record zero"),
            ("user", "record one"),
            ("user", "record two"),
            ("user", "record three"),
            ("user", "record four"),
        ],
    )
    .expect("construct");

    let view = ns.render().expect("render");
    assert_eq!(view.entries, 5);

    let receipt = ns
        .summarize_records(&view.render_id, 0, 3, "batch-1", &StubSummarizer)
        .await
        .expect("summarize");

    // Records 0–2 were replaced by one carrier, so we go from 5 to 3.
    assert_eq!(ns.stats().records, 3);
    assert_eq!(ns.stats().nodes, 1);
    assert!(!receipt.replaced_record_ids.is_empty());
}

#[tokio::test]
async fn simple_summarize_records_with_failing_summarizer_cancels_and_returns_error() {
    let mut ns = SimpleContextNamespace::from_simple_records(
        config(),
        &[
            ("user", "record zero"),
            ("user", "record one"),
            ("user", "record two"),
        ],
    )
    .expect("construct");

    let view = ns.render().expect("render");

    let result = ns
        .summarize_records(&view.render_id, 0, 2, "batch-1", &FailingSummarizer)
        .await;

    assert!(result.is_err());
    // State unchanged: still 3 records, 0 nodes.
    assert_eq!(ns.stats().records, 3);
    assert_eq!(ns.stats().nodes, 0);
}

// ---------------------------------------------------------------------------
// Expand and collapse
// ---------------------------------------------------------------------------

#[tokio::test]
async fn simple_expand_activates_node_for_ttl() {
    let mut ns = SimpleContextNamespace::from_simple_records(
        config(),
        &[
            ("user", "record zero"),
            ("user", "record one"),
            ("user", "record two"),
            ("user", "record three"),
        ],
    )
    .expect("construct");

    let view = ns.render().expect("render");
    ns.summarize_records(&view.render_id, 0, 2, "batch-1", &StubSummarizer)
        .await
        .expect("summarize");

    let receipt = ns.expand("/batch-1", 2).expect("expand");
    assert_eq!(receipt.ttl_renders, 2);
    assert!(ns.is_expanded("/batch-1"));
}

#[tokio::test]
async fn simple_collapse_clears_ttl() {
    let mut ns = SimpleContextNamespace::from_simple_records(
        config(),
        &[
            ("user", "record zero"),
            ("user", "record one"),
            ("user", "record two"),
            ("user", "record three"),
        ],
    )
    .expect("construct");

    let view = ns.render().expect("render");
    ns.summarize_records(&view.render_id, 0, 2, "batch-1", &StubSummarizer)
        .await
        .expect("summarize");

    ns.expand("/batch-1", 2).expect("expand");
    assert!(ns.is_expanded("/batch-1"));

    ns.collapse("/batch-1").expect("collapse");
    assert!(!ns.is_expanded("/batch-1"));
}

// ---------------------------------------------------------------------------
// List nodes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn simple_list_nodes_returns_all_nodes() {
    let mut ns = SimpleContextNamespace::from_simple_records(
        config(),
        &[
            ("user", "record zero"),
            ("user", "record one"),
            ("user", "record two"),
            ("user", "record three"),
            ("user", "record four"),
        ],
    )
    .expect("construct");

    let view = ns.render().expect("render");
    ns.summarize_records(&view.render_id, 0, 3, "batch-1", &StubSummarizer)
        .await
        .expect("summarize");

    let nodes = ns.list_nodes(NodeFilter::All);
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].label, "batch-1");
}

#[tokio::test]
async fn simple_list_nodes_filter_expanded() {
    let mut ns = SimpleContextNamespace::from_simple_records(
        config(),
        &[
            ("user", "record zero"),
            ("user", "record one"),
            ("user", "record two"),
            ("user", "record three"),
            ("user", "record four"),
        ],
    )
    .expect("construct");

    let view = ns.render().expect("render");
    ns.summarize_records(&view.render_id, 0, 2, "batch-1", &StubSummarizer)
        .await
        .expect("summarize");

    // Re-render: the first commit invalidated the previous render_id.
    let view2 = ns.render().expect("render");
    ns.summarize_records(&view2.render_id, 2, 4, "batch-2", &StubSummarizer)
        .await
        .expect("summarize");

    ns.expand("/batch-1", 2).expect("expand");

    let expanded = ns.list_nodes(NodeFilter::ExpandedOnly);
    assert_eq!(expanded.len(), 1);
    assert_eq!(expanded[0].label, "batch-1");

    let collapsed = ns.list_nodes(NodeFilter::CollapsedOnly);
    assert_eq!(collapsed.len(), 1);
    assert_eq!(collapsed[0].label, "batch-2");
}

// ---------------------------------------------------------------------------
// Capabilities
// ---------------------------------------------------------------------------

#[tokio::test]
async fn simple_capabilities_reports_versions_and_features() {
    let ns = SimpleContextNamespace::new(config()).expect("valid config");
    let caps = ns.capabilities();

    assert!(!caps.library_version.is_empty());
    assert!(caps.snapshot_schema > 0);
    assert!(caps.render_schema > 0);
    assert!(caps.policy_schema > 0);
    assert!(caps.features.contains("json-schema"));
    assert!(caps.features.contains("async-adapters"));
}

// ---------------------------------------------------------------------------
// Validate
// ---------------------------------------------------------------------------

#[tokio::test]
async fn simple_validate_passes_on_healthy_namespace() {
    let ns = SimpleContextNamespace::from_simple_records(
        config(),
        &[("user", "hello"), ("assistant", "hi")],
    )
    .expect("construct");

    ns.validate().expect("invariants hold");
}

// ---------------------------------------------------------------------------
// Snapshot and restore
// ---------------------------------------------------------------------------

#[tokio::test]
async fn simple_snapshot_and_restore_preserves_state() {
    let mut ns = SimpleContextNamespace::from_simple_records(
        config(),
        &[
            ("user", "record zero"),
            ("user", "record one"),
            ("user", "record two"),
            ("user", "record three"),
        ],
    )
    .expect("construct");

    let view = ns.render().expect("render");
    ns.summarize_records(&view.render_id, 0, 2, "batch-1", &StubSummarizer)
        .await
        .expect("summarize");

    let snapshot = ns.snapshot().expect("snapshot");
    let restored =
        SimpleContextNamespace::restore(snapshot.data(), Default::default()).expect("restore");

    assert_eq!(restored.version(), ns.version());
    assert_eq!(restored.stats().records, ns.stats().records);
    assert_eq!(restored.stats().nodes, ns.stats().nodes);
}

// ---------------------------------------------------------------------------
// Full lifecycle end-to-end
// ---------------------------------------------------------------------------

#[tokio::test]
async fn simple_full_lifecycle_create_append_summarize_expand_render_validate_snapshot() {
    // 1. Create
    let mut ns = SimpleContextNamespace::new(config()).expect("valid config");

    // 2. Append records
    for i in 0..8 {
        ns.append("user", &format!("Step {i}: do thing {i}."))
            .expect("append");
    }
    assert_eq!(ns.stats().records, 8);

    // 3. Render
    let view = ns.render().expect("render");
    assert_eq!(view.entries, 8);

    // 4. Summarize records 0–4
    let receipt = ns
        .summarize_records(&view.render_id, 0, 5, "batch-1", &StubSummarizer)
        .await
        .expect("summarize");
    assert_eq!(ns.stats().nodes, 1);
    assert!(!receipt.replaced_record_ids.is_empty());

    // 5. Render again to get fresh render_id
    let view2 = ns.render().expect("render");

    // 6. Summarize more records to create a sibling
    let _receipt2 = ns
        .summarize_records(&view2.render_id, 1, 3, "batch-2", &StubSummarizer)
        .await
        .expect("summarize");
    assert_eq!(ns.stats().nodes, 2);

    // 7. Expand /batch-1
    ns.expand("/batch-1", 2).expect("expand");
    assert!(ns.is_expanded("/batch-1"));

    // 8. Render with expansion active
    let _expanded_view = ns.render().expect("render with expansion");

    // 9. Collapse /batch-1
    ns.collapse("/batch-1").expect("collapse");
    assert!(!ns.is_expanded("/batch-1"));

    // 10. List nodes
    let nodes = ns.list_nodes(NodeFilter::All);
    assert_eq!(nodes.len(), 2);

    // 11. Capabilities
    let caps = ns.capabilities();
    assert!(caps.features.contains("async-adapters"));

    // 12. Validate
    ns.validate().expect("invariants hold");

    // 13. Snapshot and restore
    let snapshot = ns.snapshot().expect("snapshot");
    let restored =
        SimpleContextNamespace::restore(snapshot.data(), Default::default()).expect("restore");
    assert_eq!(restored.version(), ns.version());
    assert_eq!(restored.stats().nodes, ns.stats().nodes);

    // 14. Restored namespace validates too
    restored.validate().expect("restored invariants hold");
}
