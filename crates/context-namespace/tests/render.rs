use context_namespace::{RenderId, RenderPreview, RenderedHistory, SummaryPreview};
use uuid::Uuid;

fn render_id(n: u8) -> RenderId {
    RenderId::new(Uuid::from_fields(0, 0, 0, &[0, 0, 0, 0, 0, 0, 0, n]))
}

#[test]
fn rendered_history_stores_id_json_and_bytes() {
    let history = RenderedHistory {
        render_id: render_id(1),
        namespace_version: 0,
        json: r#"[{"role":"user","content":"hi"}]"#.to_string(),
        bytes: 30,
        entries: 1,
    };
    assert_eq!(history.render_id, render_id(1));
    assert_eq!(history.entries, 1);
    assert_eq!(history.bytes, 30);
    assert!(history.json.contains("user"));
}

#[test]
fn rendered_history_serializes() {
    let history = RenderedHistory {
        render_id: render_id(1),
        namespace_version: 0,
        json: "[]".to_string(),
        bytes: 2,
        entries: 0,
    };
    let json = serde_json::to_string(&history).expect("serialize");
    let restored: RenderedHistory = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(restored.entries, 0);
}

#[test]
fn render_preview_stores_json_and_maintenance_units() {
    let preview = RenderPreview {
        namespace_version: 3,
        json: r#"[{"content":"x"}]"#.to_string(),
        bytes: 16,
        entries: 1,
        maintenance_units: vec![],
    };
    assert_eq!(preview.namespace_version, 3);
    assert_eq!(preview.entries, 1);
    assert!(preview.maintenance_units.is_empty());
}

#[test]
fn summary_preview_stores_label_summary_and_gloss() {
    let preview = SummaryPreview {
        label: "inv".to_string(),
        summary: "explored parser".to_string(),
        gloss: "parser".to_string(),
        node_id: None,
    };
    assert_eq!(preview.label, "inv");
    assert_eq!(preview.summary, "explored parser");
    assert!(preview.node_id.is_none());
}
