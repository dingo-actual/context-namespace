"""Python exception hierarchy for the context-namespace facade.

All exceptions derive from ContextNamespaceError. Native (Rust)
exceptions raised by the _native module are caught by the facade and
re-raised as the corresponding Python exception from this module.

Important Classes:
    ContextNamespaceError: Base exception for all namespace errors.
    InvalidRecordError: A record is invalid (missing render field, etc.).
    ReservedKeyCollisionError: A record contains the reserved __ctx key.
    InvalidSelectionError: A selection range is invalid.
    StaleRenderError: The render ID is stale or unknown.
    UnknownPathError: The path or node ID does not resolve.
    PathCollisionError: A new node path collides with an existing path.
    PolicyViolationError: The policy configuration rejects the operation.
    StaleSummaryError: The summary request is stale or not found.
    InvalidSummaryError: The summary result fails validation.
    EmbeddingMismatchError: Embedding dimensions or model ID mismatch.
    SnapshotVersionError: The snapshot schema version is unsupported.
    InvariantViolationError: An invariant violation was detected.
"""

from __future__ import annotations


class ContextNamespaceError(Exception):
    """Base exception for all context-namespace errors."""


class InvalidRecordError(ContextNamespaceError):
    """Raised when a record is invalid (missing render field, wrong type)."""


class ReservedKeyCollisionError(InvalidRecordError):
    """Raised when a record contains the reserved ``__ctx`` metadata key."""


class InvalidSelectionError(ContextNamespaceError):
    """Raised when a selection range is invalid."""


class StaleRenderError(InvalidSelectionError):
    """Raised when the render ID is stale or unknown."""


class UnknownPathError(ContextNamespaceError):
    """Raised when the path or node ID does not resolve to a live node."""


class PathCollisionError(ContextNamespaceError):
    """Raised when a new node path collides with an existing path or alias."""


class PolicyViolationError(ContextNamespaceError):
    """Raised when the policy configuration rejects the operation."""


class StaleSummaryError(ContextNamespaceError):
    """Raised when the summary request is stale or not found."""


class InvalidSummaryError(ContextNamespaceError):
    """Raised when the summary result fails validation."""


class EmbeddingMismatchError(ContextNamespaceError):
    """Raised when embedding dimensions or model ID do not match."""


class SnapshotVersionError(ContextNamespaceError):
    """Raised when the snapshot schema version is unsupported."""


class InvariantViolationError(ContextNamespaceError):
    """Raised when an invariant violation is detected during validation."""
