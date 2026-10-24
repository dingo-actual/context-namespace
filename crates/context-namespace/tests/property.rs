use context_namespace::{
    ContextNamespace, DefensiveLimits, NamespaceConfig, NamespacePath, NewRecord, NodeLabel,
    RecordSelection, SummaryResult,
};
use indexmap::IndexMap;
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Config and helpers
// ---------------------------------------------------------------------------

fn test_config() -> NamespaceConfig {
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

// ---------------------------------------------------------------------------
// Operation enum — the set of operations we generate sequences from
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Op {
    Append(String),
    Render,
    SummarizeRecords { start: u32, end: u32, label: String },
    Expand { path_idx: usize, ttl: u32 },
    Snapshot,
}

/// Generates a random label string from a small alphabet.
fn label_strategy() -> impl Strategy<Value = String> {
    "[a-z]{1,8}"
}

/// Generates a random content string.
fn content_strategy() -> impl Strategy<Value = String> {
    "[a-z ]{1,20}"
}

/// Generates a sequence of operations.
fn ops_strategy() -> impl Strategy<Value = Vec<Op>> {
    prop::collection::vec(content_strategy(), 1..10).prop_flat_map(|initial_records| {
        prop::collection::vec(
            (0..20u32, 0..20u32, label_strategy(), content_strategy()).prop_map(
                move |(a, b, label, content)| {
                    // We don't use these directly; they're for the strategy
                    // to have enough entropy. The actual ops are generated
                    // dynamically below.
                    (a, b, label, content)
                },
            ),
            1..30,
        )
        .prop_map(move |_| {
            // Build a sequence dynamically, tracking state to generate
            // plausible operations.
            let mut ops = Vec::new();
            for content in &initial_records {
                ops.push(Op::Append(content.clone()));
            }

            let mut record_count = initial_records.len();
            let mut node_count = 0usize;

            // Generate a random-ish sequence. We use a simple deterministic
            // approach: alternate between render, summarize, expand, snapshot.
            let mut rng_state = record_count as u64;
            for i in 0..20 {
                rng_state = rng_state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(i as u64);
                let choice = (rng_state >> 32) % 5;
                match choice {
                    0 => {
                        // Append
                        ops.push(Op::Append(format!("rec {i}")));
                        record_count += 1;
                    }
                    1 => {
                        // Render
                        ops.push(Op::Render);
                    }
                    2 => {
                        // Summarize records (if we have enough)
                        if record_count >= 3 {
                            let start = (i % (record_count - 2)) as u32;
                            let end = start + 2;
                            ops.push(Op::SummarizeRecords {
                                start,
                                end,
                                label: format!("label{i}"),
                            });
                            // Summarization replaces records with one carrier.
                            record_count = record_count - (end - start) as usize + 1;
                            node_count += 1;
                        } else {
                            ops.push(Op::Render);
                        }
                    }
                    3 => {
                        // Expand (if we have nodes)
                        if node_count > 0 {
                            ops.push(Op::Expand {
                                path_idx: i % node_count,
                                ttl: ((i % 4) + 1) as u32,
                            });
                        } else {
                            ops.push(Op::Render);
                        }
                    }
                    _ => {
                        // Snapshot
                        ops.push(Op::Snapshot);
                    }
                }
            }
            ops
        })
    })
}

// ---------------------------------------------------------------------------
// Property: validate() passes after every operation in a valid sequence
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 100,
        ..ProptestConfig::default()
    })]

    #[test]
    fn prop_invariants_hold_after_operation_sequence(ops in ops_strategy()) {
        let mut ns = ContextNamespace::new(test_config()).expect("valid config");
        let mut last_render_id: Option<context_namespace::RenderId> = None;
        let mut node_paths: Vec<NamespacePath> = Vec::new();

        for op in &ops {
            match op {
                Op::Append(content) => {
                    let record = make_record(content);
                    let _ = ns.append(record);
                }
                Op::Render => {
                    if let Ok(view) = ns.render() {
                        last_render_id = Some(view.render_id);
                        // Collect node paths for expand operations.
                        node_paths = ns.list_nodes()
                            .into_iter()
                            .map(|d| d.path)
                            .collect();
                    }
                }
                Op::SummarizeRecords { start, end, label } => {
                    if let Some(render_id) = last_render_id {
                        let label = make_label(label);
                        let selection = RecordSelection {
                            render_id,
                            start_index: *start,
                            end_index: *end,
                        };
                        let prepared = ns.prepare_record_summary(selection, label);
                        if let Ok(prepared) = prepared {
                            let result = SummaryResult {
                                summary: "Auto summary".to_string(),
                                gloss: "Auto gloss".to_string(),
                            };
                            let _ = ns.commit_summary(prepared, result);
                            // Update node paths after commit.
                            node_paths = ns.list_nodes()
                                .into_iter()
                                .map(|d| d.path)
                                .collect();
                        }
                    }
                }
                Op::Expand { path_idx, ttl } => {
                    if let Some(path) = node_paths.get(*path_idx) {
                        let _ = ns.expand(path, *ttl);
                    }
                }
                Op::Snapshot => {
                    let _ = ns.snapshot();
                }
            }

            // The key invariant: validate() must always pass.
            ns.validate().expect("invariants must hold after every operation");
        }
    }
}

// ---------------------------------------------------------------------------
// Property: version never decreases and only increases on canonical mutation
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 50,
        ..ProptestConfig::default()
    })]

    #[test]
    fn prop_version_monotonic_after_append_and_summary(
        num_appends in 1u32..20,
        summarize_start in 0u32..18,
        summarize_end in 1u32..19,
    ) {
        let mut ns = ContextNamespace::new(test_config()).expect("valid config");

        let v0 = ns.version();

        // Append records.
        for i in 0..num_appends {
            ns.append(make_record(&format!("record {i}"))).expect("append");
        }
        let v_after_appends = ns.version();
        prop_assert_eq!(v_after_appends, v0 + num_appends as u64);

        // Render (should not change version).
        let view = ns.render().expect("render");
        prop_assert_eq!(ns.version(), v_after_appends, "render must not bump version");

        // Summarize if the range is valid.
        let end = summarize_end.min(num_appends).max(summarize_start + 1);
        let label = make_label("test-label");
        let prepared = ns.prepare_record_summary(
            RecordSelection {
                render_id: view.render_id,
                start_index: summarize_start.min(num_appends - 1),
                end_index: end,
            },
            label,
        );
        if let Ok(prepared) = prepared {
            let result = SummaryResult {
                summary: "S".to_string(),
                gloss: "G".to_string(),
            };
            ns.commit_summary(prepared, result).expect("commit");
            prop_assert!(
                ns.version() > v_after_appends,
                "commit must bump version"
            );
        }

        ns.validate().expect("invariants hold");
    }
}

// ---------------------------------------------------------------------------
// Property: snapshot round-trip preserves state
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 50,
        ..ProptestConfig::default()
    })]

    #[test]
    fn prop_snapshot_roundtrip_preserves_state(
        records in prop::collection::vec(content_strategy(), 1..15),
    ) {
        let mut ns = ContextNamespace::new(test_config()).expect("valid config");
        for content in &records {
            ns.append(make_record(content)).expect("append");
        }

        // Optionally summarize some records.
        if records.len() >= 3 {
            let view = ns.render().expect("render");
            let label = make_label("snapshot-label");
            let prepared = ns.prepare_record_summary(
                RecordSelection {
                    render_id: view.render_id,
                    start_index: 0,
                    end_index: 2,
                },
                label,
            );
            if let Ok(prepared) = prepared {
                let _ = ns.commit_summary(prepared, SummaryResult {
                    summary: "S".to_string(),
                    gloss: "G".to_string(),
                });
            }
        }

        ns.validate().expect("pre-snapshot invariants");

        let snapshot = ns.snapshot().expect("snapshot");
        let (restored, _receipt) = ContextNamespace::restore(
            snapshot.data(),
            context_namespace::RestoreOptions::default(),
        )
        .expect("restore");

        // The restored namespace must pass validation.
        restored.validate().expect("restored invariants");

        // Version must match.
        prop_assert_eq!(restored.version(), ns.version());

        // Record count must match.
        prop_assert_eq!(restored.stats().records, ns.stats().records);

        // Node count must match.
        prop_assert_eq!(restored.stats().nodes, ns.stats().nodes);
    }
}

// ---------------------------------------------------------------------------
// Property: render output is deterministic for the same state
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 50,
        ..ProptestConfig::default()
    })]

    #[test]
    fn prop_render_deterministic_for_same_state(
        records in prop::collection::vec(content_strategy(), 1..10),
    ) {
        let mut ns = ContextNamespace::new(test_config()).expect("valid config");
        for content in &records {
            ns.append(make_record(content)).expect("append");
        }

        let r1 = ns.render().expect("first render");
        // Note: render decrements TTLs, but with no expansions active,
        // the output is the same.
        let r2 = ns.render().expect("second render");

        // The JSON includes a fresh render_id UUID each time, so we
        // compare the records portion by stripping the render_id field.
        // Instead, compare entries and bytes which are deterministic.
        prop_assert_eq!(r1.entries, r2.entries, "entry count must be deterministic");
        prop_assert_eq!(r1.bytes, r2.bytes, "byte count must be deterministic");
    }
}

// ---------------------------------------------------------------------------
// Property: malformed snapshot bytes never panic
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 100,
        ..ProptestConfig::default()
    })]

    #[test]
    fn prop_malformed_snapshot_does_not_panic(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        let result = ContextNamespace::restore(
            &bytes,
            context_namespace::RestoreOptions::default(),
        );
        // Must return an error, never panic.
        prop_assert!(result.is_err(), "malformed snapshot must return error");
    }
}
