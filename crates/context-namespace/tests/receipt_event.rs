use context_namespace::{
    AppendReceipt, CancelReceipt, ExpansionReceipt, NamespaceEvent, NodeId, RecordId, RenderId,
    RenderReceipt, RequestId, RestoreReceipt, StructuralStats, SummaryReceipt,
};
use uuid::Uuid;

fn rid(n: u8) -> RecordId {
    RecordId::new(Uuid::from_fields(0, 0, 0, &[0, 0, 0, 0, 0, 0, 0, n]))
}

fn nid(n: u8) -> NodeId {
    NodeId::new(Uuid::from_fields(0, 0, 0, &[0, 0, 0, 0, 0, 0, 0, n]))
}

fn render_id(n: u8) -> RenderId {
    RenderId::new(Uuid::from_fields(0, 0, 0, &[0, 0, 0, 0, 0, 0, 0, n]))
}

fn req(n: u8) -> RequestId {
    RequestId::new(Uuid::from_fields(0, 0, 0, &[0, 0, 0, 0, 0, 0, 0, n]))
}

// ---------------------------------------------------------------------------
// AppendReceipt
// ---------------------------------------------------------------------------

#[test]
fn append_receipt_stores_id_and_visible_index() {
    let receipt = AppendReceipt {
        record_id: rid(1),
        visible_index: 0,
    };
    assert_eq!(receipt.record_id, rid(1));
    assert_eq!(receipt.visible_index, 0);
}

// ---------------------------------------------------------------------------
// SummaryReceipt
// ---------------------------------------------------------------------------

#[test]
fn summary_receipt_stores_new_node_and_carrier() {
    let receipt = SummaryReceipt {
        new_node_id: nid(1),
        carrier_record_id: rid(2),
        replaced_record_ids: vec![rid(3), rid(4), rid(5)],
        render_id: render_id(1),
    };
    assert_eq!(receipt.new_node_id, nid(1));
    assert_eq!(receipt.carrier_record_id, rid(2));
    assert_eq!(receipt.replaced_record_ids.len(), 3);
    assert_eq!(receipt.render_id, render_id(1));
}

// ---------------------------------------------------------------------------
// CancelReceipt
// ---------------------------------------------------------------------------

#[test]
fn cancel_receipt_stores_request_id() {
    let receipt = CancelReceipt {
        request_id: "req-1".to_string(),
    };
    assert_eq!(receipt.request_id, "req-1");
}

// ---------------------------------------------------------------------------
// ExpansionReceipt
// ---------------------------------------------------------------------------

#[test]
fn expansion_receipt_stores_node_and_ttl() {
    let receipt = ExpansionReceipt {
        node_id: nid(1),
        ttl_renders: 2,
        render_id: render_id(1),
    };
    assert_eq!(receipt.node_id, nid(1));
    assert_eq!(receipt.ttl_renders, 2);
}

// ---------------------------------------------------------------------------
// RenderReceipt
// ---------------------------------------------------------------------------

#[test]
fn render_receipt_stores_render_id_and_byte_count() {
    let receipt = RenderReceipt {
        render_id: render_id(1),
        bytes: 1024,
        entries: 5,
    };
    assert_eq!(receipt.render_id, render_id(1));
    assert_eq!(receipt.bytes, 1024);
    assert_eq!(receipt.entries, 5);
}

// ---------------------------------------------------------------------------
// RestoreReceipt
// ---------------------------------------------------------------------------

#[test]
fn restore_receipt_stats() {
    let receipt = RestoreReceipt {
        records: 10,
        nodes: 3,
    };
    assert_eq!(receipt.records, 10);
    assert_eq!(receipt.nodes, 3);
}

// ---------------------------------------------------------------------------
// StructuralStats
// ---------------------------------------------------------------------------

#[test]
fn structural_stats_defaults_to_zero() {
    let stats = StructuralStats::default();
    assert_eq!(stats.records, 0);
    assert_eq!(stats.nodes, 0);
    assert_eq!(stats.active_expansions, 0);
}

#[test]
fn structural_stats_stores_counts() {
    let stats = StructuralStats {
        records: 100,
        nodes: 5,
        active_expansions: 2,
        pending_requests: 3,
        alias_count: 1,
        max_depth: 3,
        dropped_events: 7,
    };
    assert_eq!(stats.records, 100);
    assert_eq!(stats.nodes, 5);
    assert_eq!(stats.active_expansions, 2);
    assert_eq!(stats.pending_requests, 3);
    assert_eq!(stats.alias_count, 1);
    assert_eq!(stats.max_depth, 3);
    assert_eq!(stats.dropped_events, 7);
}

// ---------------------------------------------------------------------------
// NamespaceEvent
// ---------------------------------------------------------------------------

#[test]
fn event_append_variant() {
    let event = NamespaceEvent::Append {
        record_id: rid(1),
        visible_index: 0,
    };
    assert!(matches!(event, NamespaceEvent::Append { .. }));
}

#[test]
fn event_summary_prepared_variant() {
    let event = NamespaceEvent::SummaryPrepared { request_id: req(1) };
    assert!(matches!(event, NamespaceEvent::SummaryPrepared { .. }));
}

#[test]
fn event_summary_committed_variant() {
    let event = NamespaceEvent::SummaryCommitted {
        node_id: nid(1),
        carrier_record_id: rid(2),
        replaced_record_ids: vec![rid(3)],
    };
    assert!(matches!(event, NamespaceEvent::SummaryCommitted { .. }));
}

#[test]
fn event_summary_cancelled_variant() {
    let event = NamespaceEvent::SummaryCancelled {
        request_id: "req-1".to_string(),
    };
    assert!(matches!(event, NamespaceEvent::SummaryCancelled { .. }));
}

#[test]
fn event_expansion_activated_variant() {
    let event = NamespaceEvent::ExpansionActivated {
        node_id: nid(1),
        ttl_renders: 2,
    };
    assert!(matches!(event, NamespaceEvent::ExpansionActivated { .. }));
}

#[test]
fn event_expansion_expired_variant() {
    let event = NamespaceEvent::ExpansionExpired { node_id: nid(1) };
    assert!(matches!(event, NamespaceEvent::ExpansionExpired { .. }));
}

#[test]
fn event_render_variant() {
    let event = NamespaceEvent::Render {
        render_id: render_id(1),
        bytes: 512,
        entries: 3,
    };
    assert!(matches!(event, NamespaceEvent::Render { .. }));
}

#[test]
fn event_snapshot_saved_variant() {
    let event = NamespaceEvent::SnapshotSaved { bytes: 2048 };
    assert!(matches!(event, NamespaceEvent::SnapshotSaved { .. }));
}

#[test]
fn event_restored_variant() {
    let event = NamespaceEvent::Restored {
        records: 10,
        nodes: 3,
    };
    assert!(matches!(event, NamespaceEvent::Restored { .. }));
}

// ---------------------------------------------------------------------------
// Bounded event queue
// ---------------------------------------------------------------------------

use context_namespace::{ContextNamespace, DefensiveLimits, NamespaceConfig, NewRecord};
use indexmap::IndexMap;

fn make_record(content: &str) -> NewRecord {
    let mut fields = IndexMap::new();
    fields.insert("content".to_string(), serde_json::json!(content));
    fields.insert("role".to_string(), serde_json::json!("user"));
    NewRecord::new(fields, "content", &DefensiveLimits::portfolio_defaults()).expect("valid record")
}

#[test]
fn drain_events_returns_events_in_order() {
    let mut ns = ContextNamespace::new(NamespaceConfig {
        render_field: "content".to_string(),
        ..Default::default()
    })
    .expect("valid");
    ns.append(make_record("a")).expect("append");
    ns.append(make_record("b")).expect("append");
    let events = ns.drain_events(100);
    assert_eq!(events.len(), 2);
    assert!(matches!(events[0], NamespaceEvent::Append { .. }));
    assert!(matches!(events[1], NamespaceEvent::Append { .. }));
}

#[test]
fn drain_events_with_limit_returns_at_most_limit() {
    let mut ns = ContextNamespace::new(NamespaceConfig {
        render_field: "content".to_string(),
        ..Default::default()
    })
    .expect("valid");
    ns.append(make_record("a")).expect("append");
    ns.append(make_record("b")).expect("append");
    ns.append(make_record("c")).expect("append");
    let events = ns.drain_events(2);
    assert_eq!(events.len(), 2);
    let rest = ns.drain_events(100);
    assert_eq!(rest.len(), 1);
}

#[test]
fn drain_events_on_empty_returns_empty_vec() {
    let mut ns = ContextNamespace::new(NamespaceConfig {
        render_field: "content".to_string(),
        ..Default::default()
    })
    .expect("valid");
    let events = ns.drain_events(10);
    assert!(events.is_empty());
}

#[test]
fn event_queue_is_bounded_drops_oldest_when_full() {
    let mut cfg = NamespaceConfig {
        render_field: "content".to_string(),
        ..Default::default()
    };
    cfg.limits.max_event_queue = std::num::NonZeroU32::new(3).expect("valid");
    let mut ns = ContextNamespace::new(cfg).expect("valid");
    for i in 0..5 {
        ns.append(make_record(&format!("r{i}"))).expect("append");
    }
    let events = ns.drain_events(100);
    assert_eq!(events.len(), 3);
}

#[test]
fn dropped_event_count_tracks_overflow() {
    let mut cfg = NamespaceConfig {
        render_field: "content".to_string(),
        ..Default::default()
    };
    cfg.limits.max_event_queue = std::num::NonZeroU32::new(2).expect("valid");
    let mut ns = ContextNamespace::new(cfg).expect("valid");
    for i in 0..5 {
        ns.append(make_record(&format!("r{i}"))).expect("append");
    }
    let stats = ns.stats();
    assert_eq!(stats.dropped_events, 3);
}
