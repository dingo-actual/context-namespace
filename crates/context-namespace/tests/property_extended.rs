//! Additional property-based tests using proptest.
//!
//! These complement the existing `tests/property.rs` with:
//! - Config validation properties (arbitrary field combinations)
//! - Record construction properties (round-trip, field preservation)
//! - Path parsing properties (JSON pointer encoding, depth tracking)
//! - Snapshot content hash properties (deterministic, content-addressed)
//! - Event queue ordering properties

use context_namespace::{
    ContentHash, ContextNamespace, DefensiveLimits, NamespaceConfig, NamespacePath, NewRecord,
    NodeLabel, RecordSelection, RestoreOptions, SummaryResult,
};
use indexmap::IndexMap;
use proptest::prelude::*;
use std::num::NonZeroU32;

// ---------------------------------------------------------------------------
// Shared fixtures
// ---------------------------------------------------------------------------

fn test_config() -> NamespaceConfig {
    NamespaceConfig {
        render_field: "content".to_string(),
        ..Default::default()
    }
}

fn limits() -> DefensiveLimits {
    DefensiveLimits::portfolio_defaults()
}

fn make_record(content: &str) -> NewRecord {
    let mut fields = IndexMap::new();
    fields.insert("role".to_string(), serde_json::json!("user"));
    fields.insert("content".to_string(), serde_json::json!(content));
    NewRecord::new(fields, "content", &limits()).expect("valid record")
}

fn make_label(s: &str) -> NodeLabel {
    NodeLabel::new(s, &limits()).expect("valid label")
}

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

fn content_strategy() -> impl Strategy<Value = String> {
    "[a-z ]{1,20}"
}

fn path_segment_strategy() -> impl Strategy<Value = String> {
    // Avoid ~ and / which have special meaning in JSON Pointer.
    "[a-z0-9_-]{1,10}"
}

/// Generates a valid NamespacePath string from 1 to 4 segments.
fn path_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec(path_segment_strategy(), 1..4).prop_map(|segs| {
        let mut s = String::new();
        for seg in &segs {
            s.push('/');
            s.push_str(seg);
        }
        s
    })
}

// ===========================================================================
// Property: config validation is deterministic for the same input
// ===========================================================================

proptest! {
    #[test]
    fn prop_config_validation_is_deterministic(
        render_field in "[a-z]{1,10}",
        trigger in 0u32..100_000,
        target in 0u32..100_000,
        budget in 0u32..100_000,
        preserve_prefix in 0u32..10,
        preserve_recent in 0u32..10,
    ) {
        let mut cfg = NamespaceConfig {
            render_field: render_field.clone(),
            ..Default::default()
        };
        cfg.policy.trigger_tokens = trigger;
        cfg.policy.target_tokens = target;
        cfg.policy.effective_budget_tokens = budget;
        cfg.policy.preserve_prefix_records = preserve_prefix;
        cfg.policy.preserve_recent_records = preserve_recent;

        let result1 = cfg.validate();
        let result2 = cfg.validate();
        prop_assert_eq!(result1.is_ok(), result2.is_ok(),
            "validation must be deterministic for the same config");
    }
}

// ===========================================================================
// Property: valid config always constructs successfully
// ===========================================================================

proptest! {
    #[test]
    fn prop_valid_config_constructs_namespace(
        content_field in "[a-z]{1,10}",
    ) {
        let cfg = NamespaceConfig {
            render_field: content_field.clone(),
            ..Default::default()
        };
        // Default config with a non-empty render_field is always valid.
        let ns = ContextNamespace::new(cfg);
        prop_assert!(ns.is_ok(), "valid config must construct");
        let ns = ns.unwrap();
        prop_assert_eq!(ns.version(), 0);
        prop_assert_eq!(ns.stats().records, 0);
    }
}

// ===========================================================================
// Property: ContentHash is deterministic and content-addressed
// ===========================================================================

proptest! {
    #[test]
    fn prop_content_hash_deterministic(data in prop::collection::vec(any::<u8>(), 0..256)) {
        let h1 = ContentHash::of(&data);
        let h2 = ContentHash::of(&data);
        prop_assert_eq!(h1.as_bytes(), h2.as_bytes(),
            "same input must produce same hash");
    }

    #[test]
    fn prop_content_hash_hex_roundtrip(data in prop::collection::vec(any::<u8>(), 0..256)) {
        let hash = ContentHash::of(&data);
        let hex = hash.to_hex();
        let restored = ContentHash::from_hex(&hex).expect("hex roundtrip");
        prop_assert_eq!(hash.as_bytes(), restored.as_bytes(),
            "to_hex / from_hex must roundtrip");
    }

    #[test]
    fn prop_content_hash_different_inputs_different_output(
        a in prop::collection::vec(any::<u8>(), 1..128),
        b in prop::collection::vec(any::<u8>(), 1..128),
    ) {
        prop_assume!(a != b);
        let ha = ContentHash::of(&a);
        let hb = ContentHash::of(&b);
        prop_assert_ne!(ha.as_bytes(), hb.as_bytes(),
            "different inputs should produce different hashes");
    }
}

// ===========================================================================
// Property: path parse / to_string round-trips for simple labels
// ===========================================================================

proptest! {
    #[test]
    fn prop_path_roundtrip(path_str in path_strategy()) {
        let parsed = NamespacePath::parse(&path_str, &limits());
        prop_assert!(parsed.is_ok(), "path must parse: {}", path_str);
        let path = parsed.unwrap();
        prop_assert_eq!(path.to_path_string(), path_str,
            "parse then to_string must roundtrip");
    }

    #[test]
    fn prop_path_depth_equals_segment_count(path_str in path_strategy()) {
        let path = NamespacePath::parse(&path_str, &limits()).expect("valid path");
        let segments: Vec<_> = path.segments().collect();
        prop_assert_eq!(path.depth(), segments.len(),
            "depth must equal segment count");
    }
}

// ===========================================================================
// Property: JSON pointer encoding round-trips
// ===========================================================================

proptest! {
    #[test]
    fn prop_json_pointer_encoding_roundtrip(
        seg in "[a-z~/]{1,10}"
    ) {
        // Construct a path with the segment, escape it, parse it back.
        let escaped = seg.replace('~', "~0").replace('/', "~1");
        let path_str = format!("/{escaped}");
        let parsed = NamespacePath::parse(&path_str, &limits());
        prop_assert!(parsed.is_ok(), "escaped path must parse: {}", path_str);
        let path = parsed.unwrap();
        let segments: Vec<_> = path.segments().collect();
        prop_assert_eq!(segments.len(), 1);
        prop_assert_eq!(segments[0], seg,
            "unescaped segment must match original");
        // to_path_string must re-escape.
        prop_assert_eq!(path.to_path_string(), path_str,
            "to_path_string must re-escape");
    }
}

// ===========================================================================
// Property: append preserves field content
// ===========================================================================

proptest! {
    #[test]
    fn prop_append_preserves_content(content in content_strategy()) {
        let mut ns = ContextNamespace::new(test_config()).expect("valid config");
        let receipt = ns.append(make_record(&content)).expect("append");
        prop_assert_eq!(receipt.visible_index, 0);
        prop_assert_eq!(ns.stats().records, 1);

        // Render and verify content is present.
        let view = ns.render().expect("render");
        prop_assert!(view.json.contains(&content),
            "rendered JSON must contain the original content");
    }
}

// ===========================================================================
// Property: version increments by exactly 1 per append
// ===========================================================================

proptest! {
    #[test]
    fn prop_version_increments_per_append(n in 1u32..50) {
        let mut ns = ContextNamespace::new(test_config()).expect("valid config");
        let v0 = ns.version();
        for i in 0..n {
            ns.append(make_record(&format!("record {i}"))).expect("append");
        }
        prop_assert_eq!(ns.version(), v0 + n as u64,
            "version must increase by exactly n after n appends");
    }
}

// ===========================================================================
// Property: render entry count equals record count (no summaries)
// ===========================================================================

proptest! {
    #[test]
    fn prop_render_entries_match_records(n in 0u32..30) {
        let mut ns = ContextNamespace::new(test_config()).expect("valid config");
        for i in 0..n {
            ns.append(make_record(&format!("record {i}"))).expect("append");
        }
        let view = ns.render().expect("render");
        prop_assert_eq!(view.entries, n,
            "entry count must equal record count with no summaries");
    }
}

// ===========================================================================
// Property: snapshot preserves version, records, and nodes
//           after multiple summarizes with random ranges
// ===========================================================================

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 30,
        ..ProptestConfig::default()
    })]

    #[test]
    fn prop_snapshot_preserves_state_after_summaries(
        contents in prop::collection::vec(content_strategy(), 3..15),
        summarize_count in 0u32..3,
    ) {
        let mut ns = ContextNamespace::new(test_config()).expect("valid config");
        for c in &contents {
            ns.append(make_record(c)).expect("append");
        }

        // Perform random summaries.
        let mut rng_state = contents.len() as u64;
        for i in 0..summarize_count {
            rng_state = rng_state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(i as u64 + 1);
            let record_count = ns.stats().records as u64;
            if record_count >= 3 {
                let start = ((rng_state >> 32) % (record_count - 1)) as u32;
                let end = start + 1 + ((rng_state >> 40) % 2) as u32;
                let view = ns.render().expect("render");
                let label = make_label(&format!("s{i}"));
                let rc = ns.stats().records;
                if let Ok(prepared) = ns.prepare_record_summary(
                    RecordSelection {
                        render_id: view.render_id,
                        start_index: start,
                        end_index: end.min(rc),
                    },
                    label,
                ) {
                    let _ = ns.commit_summary(prepared, SummaryResult {
                        summary: "S".to_string(),
                        gloss: "G".to_string(),
                    });
                }
            }
        }

        ns.validate().expect("invariants before snapshot");

        let snap = ns.snapshot().expect("snapshot");
        let (restored, _) = ContextNamespace::restore(
            snap.data(),
            RestoreOptions::default(),
        ).expect("restore");

        restored.validate().expect("restored invariants");
        prop_assert_eq!(restored.version(), ns.version());
        prop_assert_eq!(restored.stats().records, ns.stats().records);
        prop_assert_eq!(restored.stats().nodes, ns.stats().nodes);
    }
}

// ===========================================================================
// Property: event queue preserves FIFO order
// ===========================================================================

proptest! {
    #[test]
    fn prop_events_preserve_append_order(n in 1u32..20) {
        let mut cfg = test_config();
        // Ensure queue is large enough.
        cfg.limits.max_event_queue = NonZeroU32::new(256).unwrap();
        let mut ns = ContextNamespace::new(cfg).expect("valid config");

        for i in 0..n {
            ns.append(make_record(&format!("record {i}"))).expect("append");
        }

        let events = ns.drain_events(256);
        prop_assert_eq!(events.len(), n as usize,
            "must get one event per append");

        // All events should be Append variants in order.
        for (i, event) in events.iter().enumerate() {
            prop_assert!(
                matches!(event, context_namespace::NamespaceEvent::Append { visible_index, .. } if *visible_index == i as u32),
                "event {} must be Append with visible_index {}", i, i
            );
        }
    }
}

// ===========================================================================
// Property: render byte count is positive when records exist
// ===========================================================================

proptest! {
    #[test]
    fn prop_render_bytes_positive_with_records(n in 1u32..20) {
        let mut ns = ContextNamespace::new(test_config()).expect("valid config");
        for i in 0..n {
            ns.append(make_record(&format!("record {i}"))).expect("append");
        }
        let view = ns.render().expect("render");
        prop_assert!(view.bytes > 0, "render with records must have positive bytes");
        prop_assert_eq!(view.entries, n, "entries must match record count");
    }
}

// ===========================================================================
// Property: label NFC normalization is idempotent
// ===========================================================================

proptest! {
    #[test]
    fn prop_label_nfc_normalization_idempotent(s in "[a-zA-Z]{1,10}") {
        let label1 = NodeLabel::new(&s, &limits()).expect("valid label");
        let label2 = NodeLabel::new(label1.as_str(), &limits()).expect("valid label");
        prop_assert_eq!(label1.as_str(), label2.as_str(),
            "normalizing an already-normalized label must be a no-op");
    }
}

// ===========================================================================
// Property: cancel after prepare restores pending count to zero
// ===========================================================================

proptest! {
    #[test]
    fn prop_cancel_restores_pending_count(
        contents in prop::collection::vec(content_strategy(), 3..10),
        start in 0u32..7,
        end in 1u32..8,
    ) {
        let mut ns = ContextNamespace::new(test_config()).expect("valid config");
        for c in &contents {
            ns.append(make_record(c)).expect("append");
        }
        let view = ns.render().expect("render");
        let record_count = ns.stats().records;
        let s = start.min(record_count.saturating_sub(1));
        let e = end.min(record_count).max(s + 1);

        let prepared = ns.prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: s,
                end_index: e,
            },
            make_label("test"),
        );

        if let Ok(prepared) = prepared {
            prop_assert_eq!(ns.stats().pending_requests, 1,
                "one pending request after prepare");
            ns.cancel_summary(prepared.request_id).expect("cancel");
            prop_assert_eq!(ns.stats().pending_requests, 0,
                "pending count returns to zero after cancel");
        }

        ns.validate().expect("invariants hold after cancel");
    }
}

// ===========================================================================
// Property: expand then collapse leaves no active expansion
// ===========================================================================

proptest! {
    #[test]
    fn prop_expand_collapse_no_active_expansion(
        contents in prop::collection::vec(content_strategy(), 4..10),
        ttl in 1u32..4,
    ) {
        let mut ns = ContextNamespace::new(test_config()).expect("valid config");
        for c in &contents {
            ns.append(make_record(c)).expect("append");
        }

        // Create a summary node.
        let view = ns.render().expect("render");
        let prepared = ns.prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 2,
            },
            make_label("test"),
        ).expect("prepare");
        let receipt = ns.commit_summary(prepared, SummaryResult {
            summary: "S".to_string(),
            gloss: "G".to_string(),
        }).expect("commit");

        // Expand it.
        let path = ns.current_path(receipt.new_node_id).expect("path");
        ns.expand(&path, ttl).expect("expand");
        prop_assert_eq!(ns.stats().active_expansions, 1);

        // Collapse it.
        ns.collapse(receipt.new_node_id).expect("collapse");
        prop_assert_eq!(ns.stats().active_expansions, 0,
            "no active expansions after collapse");

        ns.validate().expect("invariants hold");
    }
}

// ===========================================================================
// Property: prepare with stale render_id always fails
// ===========================================================================

proptest! {
    #[test]
    fn prop_stale_render_id_prepare_fails(
        contents in prop::collection::vec(content_strategy(), 2..10),
    ) {
        let mut ns = ContextNamespace::new(test_config()).expect("valid config");
        for c in &contents {
            ns.append(make_record(c)).expect("append");
        }
        let view = ns.render().expect("render");

        // Mutate namespace to make render stale.
        ns.append(make_record("extra")).expect("append");

        let result = ns.prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: 0,
                end_index: 1,
            },
            make_label("test"),
        );
        prop_assert!(result.is_err(),
            "prepare with stale render_id must always fail");
    }
}

// ===========================================================================
// Property: record count after summary decreases correctly
// ===========================================================================

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 50,
        ..ProptestConfig::default()
    })]

    #[test]
    fn prop_record_count_after_summary(
        n_records in 3u32..15,
        start in 0u32..12,
        end in 1u32..13,
    ) {
        let mut ns = ContextNamespace::new(test_config()).expect("valid config");
        for i in 0..n_records {
            ns.append(make_record(&format!("record {i}"))).expect("append");
        }

        let initial_count = ns.stats().records;
        let view = ns.render().expect("render");

        let s = start.min(initial_count.saturating_sub(1));
        let e = end.min(initial_count).max(s + 1);
        let range_size = e - s;

        let prepared = ns.prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: s,
                end_index: e,
            },
            make_label("test"),
        );

        if let Ok(prepared) = prepared {
            ns.commit_summary(prepared, SummaryResult {
                summary: "S".to_string(),
                gloss: "G".to_string(),
            }).expect("commit");

            // After summary: initial - range_size + 1 (carrier).
            let expected = initial_count - range_size + 1;
            prop_assert_eq!(ns.stats().records, expected,
                "record count after summary of range [{}, {}) with {} records",
                s, e, initial_count);

            ns.validate().expect("invariants hold");
        }
    }
}
