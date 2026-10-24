use context_namespace::{DefensiveLimits, NewRecord, Record, RecordId};
use indexmap::IndexMap;
use uuid::Uuid;

fn limits() -> DefensiveLimits {
    DefensiveLimits::portfolio_defaults()
}

fn make_id(n: u8) -> RecordId {
    RecordId::new(Uuid::from_fields(0, 0, 0, &[0, 0, 0, 0, 0, 0, 0, n]))
}

// ---------------------------------------------------------------------------
// NewRecord
// ---------------------------------------------------------------------------

#[test]
fn new_record_accepts_mapping_with_string_render_field() {
    let mut fields = IndexMap::new();
    fields.insert("role".to_string(), serde_json::json!("user"));
    fields.insert("content".to_string(), serde_json::json!("inspect parser"));
    let record = NewRecord::new(fields, "content", &limits()).expect("valid record");
    assert_eq!(record.render_text(), "inspect parser");
}

#[test]
fn new_record_rejects_missing_render_field() {
    let mut fields = IndexMap::new();
    fields.insert("role".to_string(), serde_json::json!("user"));
    let result = NewRecord::new(fields, "content", &limits());
    assert!(result.is_err());
}

#[test]
fn new_record_rejects_non_string_render_field() {
    let mut fields = IndexMap::new();
    fields.insert("content".to_string(), serde_json::json!(42));
    let result = NewRecord::new(fields, "content", &limits());
    assert!(result.is_err());
}

#[test]
fn new_record_preserves_non_render_fields() {
    let mut fields = IndexMap::new();
    fields.insert("role".to_string(), serde_json::json!("user"));
    fields.insert("content".to_string(), serde_json::json!("hello"));
    fields.insert("meta".to_string(), serde_json::json!({"k": "v"}));
    let record = NewRecord::new(fields, "content", &limits()).expect("valid record");
    let fields = record.fields();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields.get("role"), Some(&serde_json::json!("user")));
}

#[test]
fn new_record_rejects_reserved_metadata_key() {
    let mut fields = IndexMap::new();
    fields.insert("__ctx".to_string(), serde_json::json!("evil"));
    fields.insert("content".to_string(), serde_json::json!("hello"));
    let result = NewRecord::new(fields, "content", &limits());
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Record (constructed from NewRecord via with_id)
// ---------------------------------------------------------------------------

#[test]
fn record_from_new_record_assigns_id_and_splits_render() {
    let mut fields = IndexMap::new();
    fields.insert("role".to_string(), serde_json::json!("user"));
    fields.insert("content".to_string(), serde_json::json!("hello world"));
    let new_record = NewRecord::new(fields, "content", &limits()).expect("valid");
    let id = make_id(1);
    let record = Record::from_new(new_record, id, "content");

    assert_eq!(record.id(), id);
    // The render field should be stored as segments, not in fields.
    assert!(record.fields().get("content").is_none());
    // Render segments should reconstruct the original text.
    assert_eq!(record.render_text(), "hello world");
}

#[test]
fn record_fields_exclude_render_field() {
    let mut fields = IndexMap::new();
    fields.insert("role".to_string(), serde_json::json!("assistant"));
    fields.insert("content".to_string(), serde_json::json!("response"));
    let new_record = NewRecord::new(fields, "content", &limits()).expect("valid");
    let record = Record::from_new(new_record, make_id(2), "content");
    let view_fields = record.fields();
    assert!(view_fields.get("content").is_none());
    assert_eq!(
        view_fields.get("role"),
        Some(&serde_json::json!("assistant"))
    );
}

#[test]
fn record_clone_is_equal() {
    let mut fields = IndexMap::new();
    fields.insert("role".to_string(), serde_json::json!("user"));
    fields.insert("content".to_string(), serde_json::json!("test"));
    let new_record = NewRecord::new(fields, "content", &limits()).expect("valid");
    let record = Record::from_new(new_record, make_id(3), "content");
    let cloned = record.clone();
    assert_eq!(record.id(), cloned.id());
    assert_eq!(record.render_text(), cloned.render_text());
}

#[test]
fn record_serializes_and_deserializes() {
    let mut fields = IndexMap::new();
    fields.insert("role".to_string(), serde_json::json!("user"));
    fields.insert("content".to_string(), serde_json::json!("serialize me"));
    let new_record = NewRecord::new(fields, "content", &limits()).expect("valid");
    let record = Record::from_new(new_record, make_id(4), "content");

    let json = serde_json::to_string(&record).expect("serialize");
    let restored: Record = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(restored.id(), record.id());
    assert_eq!(restored.render_text(), record.render_text());
}
