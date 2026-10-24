/// Errors returned by namespace operations.
#[derive(Debug, thiserror::Error)]
pub enum NamespaceError {
    /// A record lacks the configured string render field.
    #[error("record {record_id} lacks string field {field}")]
    MissingRenderField { record_id: String, field: String },

    /// The render ID is stale or unknown.
    #[error("render {0} is stale or unknown")]
    StaleRender(String),

    /// The summary request is stale or not found.
    #[error("summary request {0} is stale")]
    StaleSummary(String),

    /// The path or node ID does not resolve to a live node.
    #[error("unknown namespace path {0}")]
    UnknownPath(String),

    /// A new node path collides with an existing path or alias.
    #[error("path collision at {0}")]
    PathCollision(String),

    /// The selection range is invalid.
    #[error("selection is invalid: {0}")]
    InvalidSelection(String),

    /// The policy configuration rejects the operation.
    #[error("policy violation: {0}")]
    PolicyViolation(String),

    /// Embedding dimensions or model ID mismatch.
    #[error("embedding mismatch: {0}")]
    EmbeddingMismatch(String),

    /// The snapshot schema version is unsupported.
    #[error("snapshot schema {found} is unsupported")]
    SnapshotVersion { found: u32 },

    /// An invariant violation was detected.
    #[error("invariant violation: {0}")]
    Invariant(String),

    /// A defensive limit was exceeded.
    #[error("limit exceeded: {0}")]
    LimitExceeded(String),

    /// A configuration error.
    #[error("configuration error: {0}")]
    Config(String),

    /// The summary result fails validation.
    #[error("invalid summary result: {0}")]
    InvalidSummary(String),
}

/// Errors from high-level async workflows that wrap inference + core operations.
#[derive(Debug, thiserror::Error)]
pub enum WorkflowError<E: std::error::Error + Send + Sync + 'static> {
    /// An inference error from the summarizer or embedder.
    #[error("inference error: {0}")]
    Inference(#[source] E),

    /// A namespace core error.
    #[error("namespace error: {0}")]
    Core(#[from] NamespaceError),
}
