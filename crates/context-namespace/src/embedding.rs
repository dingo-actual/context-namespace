use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

use crate::id::NodeId;

/// Specifies the embedding model and vector dimensions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmbeddingModelSpec {
    /// The model identifier string.
    pub model_id: String,
    /// The number of dimensions in the embedding vector.
    pub dimensions: NonZeroU32,
}

/// A candidate node for embedding generation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmbeddingCandidate {
    /// The node ID to embed.
    pub node_id: NodeId,
    /// The content hash of the text to embed.
    pub content_hash: String,
    /// The text to send to the embedder.
    pub text: String,
}

/// A batch of embedding candidates for a single model.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmbeddingBatch {
    /// The embedding model specification.
    pub model: EmbeddingModelSpec,
    /// The candidates to embed.
    pub candidates: Vec<EmbeddingCandidate>,
}

/// Options controlling vector search behavior.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchOptions {
    /// Maximum number of hits to return.
    pub max_hits: usize,
    /// Minimum cosine similarity score for a hit.
    pub min_score: f32,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            max_hits: 10,
            min_score: 0.0,
        }
    }
}

/// A single search hit from vector search.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchHit {
    /// The node ID of the hit.
    pub node_id: NodeId,
    /// The cosine similarity score.
    pub score: f32,
}
