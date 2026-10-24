use context_namespace::{
    ChildDescriptor, MaintenanceDecision, MaintenanceInput, NodeId, RecordLocator, RecordSelection,
    RecordTokenEstimate, SummaryInput, SummaryResult, TextSelection,
};
use context_namespace::{ContentHash, RecordId, RenderId, RequestId};
use uuid::Uuid;

fn rid(n: u8) -> RecordId {
    RecordId::new(Uuid::from_fields(0, 0, 0, &[0, 0, 0, 0, 0, 0, 0, n]))
}

fn nid(n: u8) -> NodeId {
    NodeId::new(Uuid::from_fields(0, 0, 0, &[0, 0, 0, 0, 0, 0, 0, n]))
}

fn req(n: u8) -> RequestId {
    RequestId::new(Uuid::from_fields(0, 0, 0, &[0, 0, 0, 0, 0, 0, 0, n]))
}

fn render_id(n: u8) -> RenderId {
    RenderId::new(Uuid::from_fields(0, 0, 0, &[0, 0, 0, 0, 0, 0, 0, n]))
}

fn make_render_id(n: u8) -> RenderId {
    render_id(n)
}

// ---------------------------------------------------------------------------
// TextSelection
// ---------------------------------------------------------------------------

#[test]
fn text_selection_stores_record_locator_and_char_range() {
    let sel = TextSelection {
        record: RecordLocator::Id(rid(1)),
        start_char: 0,
        end_char: 10,
    };
    assert_eq!(sel.start_char, 0);
    assert_eq!(sel.end_char, 10);
    assert!(matches!(sel.record, RecordLocator::Id(_)));
}

#[test]
fn text_selection_with_visible_locator() {
    let sel = TextSelection {
        record: RecordLocator::Visible {
            render_id: make_render_id(1),
            index: 3,
        },
        start_char: 5,
        end_char: 20,
    };
    assert_eq!(sel.start_char, 5);
    if let RecordLocator::Visible {
        render_id: rid,
        index,
    } = sel.record
    {
        assert_eq!(index, 3);
        assert_eq!(rid, make_render_id(1));
    } else {
        panic!("expected Visible locator");
    }
}

// ---------------------------------------------------------------------------
// RecordSelection
// ---------------------------------------------------------------------------

#[test]
fn record_selection_stores_render_id_and_range() {
    let sel = RecordSelection {
        render_id: make_render_id(1),
        start_index: 2,
        end_index: 5,
    };
    assert_eq!(sel.start_index, 2);
    assert_eq!(sel.end_index, 5);
    assert_eq!(sel.render_id, make_render_id(1));
}

// ---------------------------------------------------------------------------
// SummaryInput
// ---------------------------------------------------------------------------

#[test]
fn summary_input_stores_request_id_source_and_children() {
    let input = SummaryInput {
        request_id: req(1),
        source: "combined text".to_string(),
        children: vec![ChildDescriptor {
            node_id: nid(1),
            label: "child1".to_string(),
            summary: "child summary".to_string(),
            gloss: "child gloss".to_string(),
        }],
        max_summary_chars: 1200,
        max_gloss_chars: 160,
    };
    assert_eq!(input.request_id, req(1));
    assert_eq!(input.source, "combined text");
    assert_eq!(input.children.len(), 1);
    assert_eq!(input.max_summary_chars, 1200);
}

// ---------------------------------------------------------------------------
// SummaryResult
// ---------------------------------------------------------------------------

#[test]
fn summary_result_stores_summary_and_gloss() {
    let result = SummaryResult {
        summary: "The agent explored the parser".to_string(),
        gloss: "parser exploration".to_string(),
    };
    assert_eq!(result.summary, "The agent explored the parser");
    assert_eq!(result.gloss, "parser exploration");
}

// ---------------------------------------------------------------------------
// ChildDescriptor
// ---------------------------------------------------------------------------

#[test]
fn child_descriptor_stores_fields() {
    let child = ChildDescriptor {
        node_id: nid(1),
        label: "sub".to_string(),
        summary: "sub summary".to_string(),
        gloss: "sub gloss".to_string(),
    };
    assert_eq!(child.node_id, nid(1));
    assert_eq!(child.label, "sub");
}

// ---------------------------------------------------------------------------
// MaintenanceInput / MaintenanceDecision
// ---------------------------------------------------------------------------

#[test]
fn maintenance_input_stores_estimates() {
    let input = MaintenanceInput {
        estimated_next_tokens: 25_000,
        records: vec![RecordTokenEstimate {
            record_id: rid(1),
            render_digest: ContentHash::of(b"test"),
            estimated_tokens: 500,
        }],
    };
    assert_eq!(input.estimated_next_tokens, 25_000);
    assert_eq!(input.records.len(), 1);
    assert_eq!(input.records[0].estimated_tokens, 500);
}

#[test]
fn maintenance_decision_no_action_variant() {
    let decision = MaintenanceDecision::NoAction {
        stats: context_namespace::StructuralStats::default(),
    };
    assert!(matches!(decision, MaintenanceDecision::NoAction { .. }));
}

// ---------------------------------------------------------------------------
// RecordTokenEstimate
// ---------------------------------------------------------------------------

#[test]
fn record_token_estimate_stores_fields() {
    let est = RecordTokenEstimate {
        record_id: rid(1),
        render_digest: ContentHash::of(b"digest"),
        estimated_tokens: 300,
    };
    assert_eq!(est.record_id, rid(1));
    assert_eq!(est.estimated_tokens, 300);
}

// ---------------------------------------------------------------------------
// RecordLocator
// ---------------------------------------------------------------------------

#[test]
fn record_locator_id_variant() {
    let loc = RecordLocator::Id(rid(5));
    assert!(matches!(loc, RecordLocator::Id(_)));
}

#[test]
fn record_locator_visible_variant() {
    let loc = RecordLocator::Visible {
        render_id: make_render_id(2),
        index: 7,
    };
    if let RecordLocator::Visible {
        render_id: rid,
        index,
    } = loc
    {
        assert_eq!(index, 7);
        assert_eq!(rid, make_render_id(2));
    } else {
        panic!("expected Visible locator");
    }
}
