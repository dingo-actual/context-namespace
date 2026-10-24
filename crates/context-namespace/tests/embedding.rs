#![cfg(feature = "semantic-search")]

use context_namespace::{
    EmbeddingBatch, EmbeddingCandidate, EmbeddingModelSpec, NodeId, SearchHit, SearchOptions,
};
use uuid::Uuid;

fn nid(n: u8) -> NodeId {
    NodeId::new(Uuid::from_fields(0, 0, 0, &[0, 0, 0, 0, 0, 0, 0, n]))
}

#[test]
fn embedding_model_spec_stores_id_and_dimensions() {
    let spec = EmbeddingModelSpec {
        model_id: "text-embedding-3-small".to_string(),
        dimensions: std::num::NonZeroU32::new(512).unwrap(),
    };
    assert_eq!(spec.model_id, "text-embedding-3-small");
    assert_eq!(spec.dimensions.get(), 512);
}

#[test]
fn embedding_candidate_stores_node_id_and_hash() {
    let candidate = EmbeddingCandidate {
        node_id: nid(1),
        content_hash: "abc123".to_string(),
        text: "summary text".to_string(),
    };
    assert_eq!(candidate.node_id, nid(1));
    assert_eq!(candidate.text, "summary text");
}

#[test]
fn embedding_batch_stores_candidates_and_model_spec() {
    let batch = EmbeddingBatch {
        model: EmbeddingModelSpec {
            model_id: "m1".to_string(),
            dimensions: std::num::NonZeroU32::new(128).unwrap(),
        },
        candidates: vec![
            EmbeddingCandidate {
                node_id: nid(1),
                content_hash: "h1".to_string(),
                text: "t1".to_string(),
            },
            EmbeddingCandidate {
                node_id: nid(2),
                content_hash: "h2".to_string(),
                text: "t2".to_string(),
            },
        ],
    };
    assert_eq!(batch.candidates.len(), 2);
}

#[test]
fn search_options_defaults_to_reasonable_limits() {
    let opts = SearchOptions::default();
    assert_eq!(opts.max_hits, 10);
    assert!(opts.min_score >= 0.0);
}

#[test]
fn search_hit_stores_node_id_and_score() {
    let hit = SearchHit {
        node_id: nid(1),
        score: 0.95,
    };
    assert_eq!(hit.node_id, nid(1));
    assert!((hit.score - 0.95).abs() < f32::EPSILON);
}
