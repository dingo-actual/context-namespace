use context_namespace::{
    ContentHash, DefensiveLimits, NamespaceNode, NodeId, NodeLabel, NodeSource, Provenance,
    RecordId,
};
use uuid::Uuid;

fn limits() -> DefensiveLimits {
    DefensiveLimits::portfolio_defaults()
}

fn nid(n: u8) -> NodeId {
    NodeId::new(Uuid::from_fields(0, 0, 0, &[0, 0, 0, 0, 0, 0, 0, n]))
}

fn rid(n: u8) -> RecordId {
    RecordId::new(Uuid::from_fields(0, 0, 0, &[0, 0, 0, 0, 0, 0, 0, n]))
}

fn provenance() -> Provenance {
    Provenance {
        source_hash: ContentHash::of(b"source"),
        result_hash: ContentHash::of(b"result"),
        namespace_version: 1,
    }
}

// ---------------------------------------------------------------------------
// NodeSource
// ---------------------------------------------------------------------------

#[test]
fn node_source_text_slice_variant_is_constructible() {
    let source = NodeSource::TextSlice {
        record_id: rid(1),
        segments: vec![],
    };
    assert!(matches!(source, NodeSource::TextSlice { .. }));
}

#[test]
fn node_source_record_range_variant_is_constructible() {
    let source = NodeSource::RecordRange { records: vec![] };
    assert!(matches!(source, NodeSource::RecordRange { .. }));
}

// ---------------------------------------------------------------------------
// NamespaceNode
// ---------------------------------------------------------------------------

#[test]
fn node_construction_stores_fields() {
    let label = NodeLabel::new("investigation", &limits()).expect("valid label");
    let node = NamespaceNode::new(
        nid(1),
        label,
        "Summary of initial exploration".to_string(),
        "investigation".to_string(),
        NodeSource::RecordRange { records: vec![] },
        provenance(),
    );

    assert_eq!(node.id(), nid(1));
    assert_eq!(node.summary_text(), "Summary of initial exploration");
    assert_eq!(node.gloss_text(), "investigation");
    assert!(matches!(node.source(), NodeSource::RecordRange { .. }));
    assert!(node.parent().is_none());
    assert!(node.children().is_empty());
}

#[test]
fn node_construction_with_text_slice_source() {
    let label = NodeLabel::new("sub", &limits()).expect("valid label");
    let node = NamespaceNode::new(
        nid(2),
        label,
        "s".to_string(),
        "g".to_string(),
        NodeSource::TextSlice {
            record_id: rid(1),
            segments: vec![],
        },
        provenance(),
    );
    assert!(matches!(node.source(), NodeSource::TextSlice { .. }));
}

#[test]
fn node_serializes_and_deserializes() {
    let label = NodeLabel::new("inv", &limits()).expect("valid label");
    let node = NamespaceNode::new(
        nid(1),
        label,
        "summary".to_string(),
        "gloss".to_string(),
        NodeSource::RecordRange { records: vec![] },
        provenance(),
    );
    let json = serde_json::to_string(&node).expect("serialize");
    let restored: NamespaceNode = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(restored.id(), node.id());
    assert_eq!(restored.summary_text(), node.summary_text());
}

#[test]
fn node_label_returns_stored_label() {
    let label = NodeLabel::new("mylabel", &limits()).expect("valid label");
    let node = NamespaceNode::new(
        nid(1),
        label.clone(),
        "s".to_string(),
        "g".to_string(),
        NodeSource::RecordRange { records: vec![] },
        provenance(),
    );
    assert_eq!(node.label(), &label);
}

#[test]
fn node_provenance_stored_and_accessible() {
    let prov = provenance();
    let node = NamespaceNode::new(
        nid(1),
        NodeLabel::new("x", &limits()).expect("valid"),
        "s".to_string(),
        "g".to_string(),
        NodeSource::RecordRange { records: vec![] },
        prov.clone(),
    );
    assert_eq!(node.provenance().namespace_version, 1);
    assert_eq!(
        node.provenance().source_hash.to_hex(),
        prov.source_hash.to_hex()
    );
}
