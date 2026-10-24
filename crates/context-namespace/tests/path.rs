use context_namespace::{DefensiveLimits, NamespacePath, NodeId, NodeLabel, NodeLocator};
use uuid::Uuid;

fn limits() -> DefensiveLimits {
    DefensiveLimits::portfolio_defaults()
}

// ---------------------------------------------------------------------------
// NodeLabel
// ---------------------------------------------------------------------------

#[test]
fn label_accepts_simple_ascii_text() {
    let label = NodeLabel::new("investigation", &limits()).expect("valid label");
    assert_eq!(label.as_str(), "investigation");
}

#[test]
fn label_rejects_empty_string() {
    let result = NodeLabel::new("", &limits());
    assert!(result.is_err());
}

#[test]
fn label_rejects_control_characters() {
    let result = NodeLabel::new("bad\x00label", &limits());
    assert!(result.is_err());
}

#[test]
fn label_normalizes_nfc_input() {
    // "é" can be represented as a single codepoint (U+00E9) or as "e" +
    // combining accent (U+0065 U+0301). NFC should produce the single
    // codepoint form.
    let composed = "é";
    let decomposed = "e\u{0301}";
    let label_composed = NodeLabel::new(composed, &limits()).expect("valid label");
    let label_decomposed = NodeLabel::new(decomposed, &limits()).expect("valid label");
    assert_eq!(label_composed.as_str(), label_decomposed.as_str());
    assert_eq!(label_composed.as_str(), composed);
}

#[test]
fn label_rejects_string_exceeding_max_length() {
    let mut tiny_limits = limits();
    tiny_limits.max_label_length = std::num::NonZeroU32::new(5).expect("valid nonzero");
    let result = NodeLabel::new("too_long_label", &tiny_limits);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// NamespacePath
// ---------------------------------------------------------------------------

#[test]
fn path_root_has_zero_depth_and_renders_as_slash() {
    let root = NamespacePath::root();
    assert_eq!(root.depth(), 0);
    assert_eq!(root.to_path_string(), "/");
    assert_eq!(root.segments().count(), 0);
}

#[test]
fn path_parse_accepts_single_segment() {
    let path = NamespacePath::parse("/task", &limits()).expect("valid path");
    assert_eq!(path.depth(), 1);
    assert_eq!(path.to_path_string(), "/task");
    let segments: Vec<&str> = path.segments().collect();
    assert_eq!(segments, vec!["task"]);
}

#[test]
fn path_parse_accepts_multiple_segments() {
    let path = NamespacePath::parse("/task/investigation-3", &limits()).expect("valid path");
    assert_eq!(path.depth(), 2);
    assert_eq!(path.to_path_string(), "/task/investigation-3");
    let segments: Vec<&str> = path.segments().collect();
    assert_eq!(segments, vec!["task", "investigation-3"]);
}

#[test]
fn path_parse_rejects_empty_string() {
    let result = NamespacePath::parse("", &limits());
    assert!(result.is_err());
}

#[test]
fn path_parse_rejects_relative_path_without_leading_slash() {
    let result = NamespacePath::parse("task", &limits());
    assert!(result.is_err());
}

#[test]
fn path_parse_treats_trailing_slash_as_root() {
    // A trailing slash should not produce an empty segment.
    let path = NamespacePath::parse("/task/", &limits()).expect("valid path");
    assert_eq!(path.depth(), 1);
    let segments: Vec<&str> = path.segments().collect();
    assert_eq!(segments, vec!["task"]);
}

#[test]
fn path_parse_unescapes_json_pointer_encoding() {
    // JSON Pointer: ~1 means /, ~0 means ~
    let path = NamespacePath::parse("/a~1b~0c", &limits()).expect("valid path");
    let segments: Vec<&str> = path.segments().collect();
    assert_eq!(segments, vec!["a/b~c"]);
}

#[test]
fn path_to_string_escapes_json_pointer_encoding() {
    let path = NamespacePath::parse("/a~1b~0c", &limits()).expect("valid path");
    assert_eq!(path.to_path_string(), "/a~1b~0c");
}

#[test]
fn path_child_appends_label() {
    let parent = NamespacePath::parse("/task", &limits()).expect("valid path");
    let label = NodeLabel::new("sub", &limits()).expect("valid label");
    let child = parent.child(&label, &limits()).expect("valid child");
    assert_eq!(child.depth(), 2);
    assert_eq!(child.to_path_string(), "/task/sub");
}

#[test]
fn path_child_rejects_exceeding_depth_limit() {
    let mut tiny_limits = limits();
    tiny_limits.max_tree_depth = std::num::NonZeroU32::new(1).expect("valid nonzero");
    let parent = NamespacePath::parse("/task", &tiny_limits).expect("valid path");
    let label = NodeLabel::new("sub", &tiny_limits).expect("valid label");
    let result = parent.child(&label, &tiny_limits);
    assert!(result.is_err());
}

#[test]
fn path_eq_and_hash() {
    let a = NamespacePath::parse("/a/b", &limits()).expect("valid");
    let b = NamespacePath::parse("/a/b", &limits()).expect("valid");
    assert_eq!(a, b);

    let mut set = std::collections::HashSet::new();
    set.insert(a);
    assert!(set.contains(&b));
}

#[test]
fn path_display_matches_to_string() {
    let path = NamespacePath::parse("/a/b/c", &limits()).expect("valid");
    assert_eq!(format!("{path}"), path.to_path_string());
}

// ---------------------------------------------------------------------------
// NodeLocator
// ---------------------------------------------------------------------------

#[test]
fn node_locator_from_path() {
    let path = NamespacePath::parse("/task", &limits()).expect("valid path");
    let locator = NodeLocator::from_path(path.clone());
    match locator {
        NodeLocator::Path(p) => assert_eq!(p, path),
        NodeLocator::Id(_) => panic!("expected Path variant"),
    }
}

#[test]
fn node_locator_from_id() {
    let id = NodeId::new(Uuid::nil());
    let locator = NodeLocator::from_id(id);
    match locator {
        NodeLocator::Path(_) => panic!("expected Id variant"),
        NodeLocator::Id(i) => assert_eq!(i, id),
    }
}
