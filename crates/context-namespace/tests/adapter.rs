use context_namespace::{
    ContextNamespace, DefensiveLimits, MaintenanceInput, MaintenanceOutcome, NamespaceConfig,
    NewRecord, NodeLabel, NodeLocator, RecordLocator, RecordSelection, RecordTokenEstimate,
    SummaryInput, SummaryResult, TextSelection, WorkflowError,
};
use indexmap::IndexMap;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU32, Ordering};

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

// ---------------------------------------------------------------------------
// Mock summarizer
// ---------------------------------------------------------------------------

struct MockSummarizer {
    summary: String,
    gloss: String,
}

#[async_trait::async_trait]
impl context_namespace::Summarizer for MockSummarizer {
    type Error = std::convert::Infallible;

    async fn summarize(&self, _input: SummaryInput) -> Result<SummaryResult, Self::Error> {
        Ok(SummaryResult {
            summary: self.summary.clone(),
            gloss: self.gloss.clone(),
        })
    }
}

struct FailingSummarizer;

#[derive(Debug, thiserror::Error)]
enum FailingError {
    #[error("inference failed")]
    InferenceFailed,
}

#[async_trait::async_trait]
impl context_namespace::Summarizer for FailingSummarizer {
    type Error = FailingError;

    async fn summarize(&self, _input: SummaryInput) -> Result<SummaryResult, Self::Error> {
        Err(FailingError::InferenceFailed)
    }
}

/// A summarizer that counts how many times it was called.
struct CountingSummarizer {
    summary: String,
    gloss: String,
    call_count: AtomicU32,
}

#[async_trait::async_trait]
impl context_namespace::Summarizer for CountingSummarizer {
    type Error = std::convert::Infallible;

    async fn summarize(&self, _input: SummaryInput) -> Result<SummaryResult, Self::Error> {
        self.call_count.fetch_add(1, Ordering::Relaxed);
        Ok(SummaryResult {
            summary: self.summary.clone(),
            gloss: self.gloss.clone(),
        })
    }
}

// ===========================================================================
// summarize_records_with tests
// ===========================================================================

#[tokio::test]
async fn summarize_records_with_commits_summary() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["alpha", "beta", "gamma", "delta"]);
    let view = ns.render().expect("render");

    let summarizer = MockSummarizer {
        summary: "Summary of records 1-2".to_string(),
        gloss: "beta and gamma".to_string(),
    };
    let receipt = ns
        .summarize_records_with(
            RecordSelection {
                render_id: view.render_id,
                start_index: 1,
                end_index: 3,
            },
            make_label("batch1"),
            &summarizer,
        )
        .await
        .expect("workflow");

    assert!(!receipt.replaced_record_ids.is_empty());
    ns.validate().expect("invariants hold");
}

#[tokio::test]
async fn summarize_records_with_inference_failure_cancels_and_preserves_state() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["alpha", "beta", "gamma"]);
    let view = ns.render().expect("render");

    let version_before = ns.version();
    let pending_before = ns.stats().pending_requests;

    let result = ns
        .summarize_records_with(
            RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 2,
            },
            make_label("batch1"),
            &FailingSummarizer,
        )
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        WorkflowError::Inference(e) => {
            assert_eq!(e.to_string(), "inference failed");
        }
        other => panic!("expected Inference error, got {other:?}"),
    }
    assert_eq!(ns.version(), version_before, "version must not change");
    assert_eq!(
        ns.stats().pending_requests,
        pending_before,
        "pending request must be cleaned up"
    );
    ns.validate().expect("invariants hold");
}

#[tokio::test]
async fn summarize_records_with_calls_summarizer_once() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["alpha", "beta"]);
    let view = ns.render().expect("render");

    let summarizer = CountingSummarizer {
        summary: "S".to_string(),
        gloss: "G".to_string(),
        call_count: AtomicU32::new(0),
    };
    ns.summarize_records_with(
        RecordSelection {
            render_id: view.render_id,
            start_index: 0,
            end_index: 1,
        },
        make_label("s1"),
        &summarizer,
    )
    .await
    .expect("workflow");

    assert_eq!(summarizer.call_count.load(Ordering::Relaxed), 1);
}

// ===========================================================================
// summarize_text_with tests
// ===========================================================================

#[tokio::test]
async fn summarize_text_with_commits_summary() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["hello world this is a long record"]);
    let view = ns.render().expect("render");

    let summarizer = MockSummarizer {
        summary: "Shortened text".to_string(),
        gloss: "Text slice".to_string(),
    };
    let receipt = ns
        .summarize_text_with(
            TextSelection {
                record: RecordLocator::Visible {
                    render_id: view.render_id,
                    index: 0,
                },
                start_char: 0,
                end_char: 11,
            },
            make_label("slice1"),
            &summarizer,
        )
        .await
        .expect("workflow");

    assert!(!receipt.replaced_record_ids.is_empty());
    ns.validate().expect("invariants hold");
}

#[tokio::test]
async fn summarize_text_with_inference_failure_cancels_and_preserves_state() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["hello world this is long"]);
    let view = ns.render().expect("render");

    let version_before = ns.version();

    let result = ns
        .summarize_text_with(
            TextSelection {
                record: RecordLocator::Visible {
                    render_id: view.render_id,
                    index: 0,
                },
                start_char: 0,
                end_char: 5,
            },
            make_label("slice1"),
            &FailingSummarizer,
        )
        .await;

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), WorkflowError::Inference(_)));
    assert_eq!(ns.version(), version_before, "version must not change");
    assert_eq!(ns.stats().pending_requests, 0);
    ns.validate().expect("invariants hold");
}

// ===========================================================================
// maintain_with tests
// ===========================================================================

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

#[tokio::test]
async fn maintain_with_no_action_when_below_trigger() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["alpha", "beta"]);

    let preview = ns.preview_render().expect("preview");
    let input = input_from_preview(&preview, 100, 50);

    let summarizer = CountingSummarizer {
        summary: "S".to_string(),
        gloss: "G".to_string(),
        call_count: AtomicU32::new(0),
    };
    let outcome = ns
        .maintain_with(input, &summarizer)
        .await
        .expect("workflow");

    assert!(matches!(outcome, MaintenanceOutcome::NoAction { .. }));
    assert_eq!(
        summarizer.call_count.load(Ordering::Relaxed),
        0,
        "summarizer must not be called when below trigger"
    );
}

#[tokio::test]
async fn maintain_with_summarizes_when_above_trigger() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(
        &mut ns,
        &[
            "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa",
            "lambda", "mu",
        ],
    );

    let preview = ns.preview_render().expect("preview");
    // 50_000 total (above trigger of 24_000), 5_000 per record
    let input = input_from_preview(&preview, 50_000, 5_000);

    let summarizer = CountingSummarizer {
        summary: "Maintenance summary".to_string(),
        gloss: "Auto summary".to_string(),
        call_count: AtomicU32::new(0),
    };
    let outcome = ns
        .maintain_with(input, &summarizer)
        .await
        .expect("workflow");

    assert!(matches!(outcome, MaintenanceOutcome::Summarized(_)));
    assert_eq!(
        summarizer.call_count.load(Ordering::Relaxed),
        1,
        "summarizer must be called exactly once"
    );
    ns.validate().expect("invariants hold");
}

#[tokio::test]
async fn maintain_with_inference_failure_cancels_and_preserves_state() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(
        &mut ns,
        &[
            "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa",
            "lambda", "mu",
        ],
    );

    let preview = ns.preview_render().expect("preview");
    let input = input_from_preview(&preview, 50_000, 5_000);

    let version_before = ns.version();

    let result = ns.maintain_with(input, &FailingSummarizer).await;

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), WorkflowError::Inference(_)));
    assert_eq!(ns.version(), version_before, "version must not change");
    assert_eq!(ns.stats().pending_requests, 0);
    ns.validate().expect("invariants hold");
}

// ===========================================================================
// expand_node tests
// ===========================================================================

#[test]
fn expand_node_by_path_activates_expansion() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["alpha", "beta", "gamma", "delta"]);
    let view = ns.render().expect("render");

    let prepared = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 2,
            },
            make_label("batch1"),
        )
        .expect("prepare");
    let receipt = ns
        .commit_summary(
            prepared,
            SummaryResult {
                summary: "S".to_string(),
                gloss: "G".to_string(),
            },
        )
        .expect("commit");

    let path = ns.current_path(receipt.new_node_id).expect("path");
    let locator = NodeLocator::from_path(path);
    let result = ns.expand_node(locator, NonZeroU32::new(2).unwrap());

    assert!(result.is_ok());
    let exp_receipt = result.unwrap();
    assert_eq!(exp_receipt.ttl_renders, 2);
}

#[test]
fn expand_node_by_id_activates_expansion() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["alpha", "beta", "gamma"]);
    let view = ns.render().expect("render");

    let prepared = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 2,
            },
            make_label("batch1"),
        )
        .expect("prepare");
    let receipt = ns
        .commit_summary(
            prepared,
            SummaryResult {
                summary: "S".to_string(),
                gloss: "G".to_string(),
            },
        )
        .expect("commit");

    let locator = NodeLocator::from_id(receipt.new_node_id);
    let result = ns.expand_node(locator, NonZeroU32::new(3).unwrap());

    assert!(result.is_ok());
    assert_eq!(result.unwrap().ttl_renders, 3);
}

#[test]
fn expand_node_unknown_id_returns_error() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["alpha"]);

    let unknown_id = context_namespace::NodeId::new(uuid::Uuid::nil());
    let result = ns.expand_node(
        NodeLocator::from_id(unknown_id),
        NonZeroU32::new(1).unwrap(),
    );
    assert!(result.is_err());
}

#[test]
fn expand_node_ttl_exceeding_max_returns_error() {
    let mut ns = ContextNamespace::new(config()).expect("valid config");
    append_records(&mut ns, &["alpha", "beta"]);
    let view = ns.render().expect("render");

    let prepared = ns
        .prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 1,
            },
            make_label("batch1"),
        )
        .expect("prepare");
    let receipt = ns
        .commit_summary(
            prepared,
            SummaryResult {
                summary: "S".to_string(),
                gloss: "G".to_string(),
            },
        )
        .expect("commit");

    let path = ns.current_path(receipt.new_node_id).expect("path");
    let locator = NodeLocator::from_path(path);
    // Default max_expansion_ttl is 4; try 5.
    let result = ns.expand_node(locator, NonZeroU32::new(5).unwrap());
    assert!(result.is_err());
}
