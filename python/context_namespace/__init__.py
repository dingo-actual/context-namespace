"""Python facade for the context-namespace Rust library.

This package provides a thin, stdlib-only Python layer over the native
extension module. It re-exports the public API so callers never need to
import from ``_native`` directly.
"""

from __future__ import annotations

from importlib.metadata import version as _dist_version

__version__ = _dist_version("context-namespace")

from .config import (
    CarrierTemplate,
    DefensiveLimits,
    IndexMode,
    NamespaceConfig,
    PolicyConfig,
    RenderOptions,
)
from .errors import (
    ContextNamespaceError,
    EmbeddingMismatchError,
    InvalidRecordError,
    InvalidSelectionError,
    InvalidSummaryError,
    InvariantViolationError,
    PathCollisionError,
    PolicyViolationError,
    ReservedKeyCollisionError,
    SnapshotVersionError,
    StaleRenderError,
    StaleSummaryError,
    UnknownPathError,
)
from .facade import ContextNamespace
from .models import (
    AppendReceipt,
    Capabilities,
    ChildDescriptor,
    EmbeddingModelSpec,
    ExpansionReceipt,
    MaintenanceEstimate,
    MaintenanceReceipt,
    MaintenanceUnitPreview,
    NodeDescriptor,
    RecordTokenEstimate,
    RenderedHistory,
    RenderPreview,
    StructuralStats,
    SummaryInput,
    SummaryReceipt,
    SummaryResult,
)
from .protocols import Embedder, Summarizer

__all__ = [
    "AppendReceipt",
    "Capabilities",
    "CarrierTemplate",
    "ChildDescriptor",
    "ContextNamespace",
    "ContextNamespaceError",
    "DefensiveLimits",
    "Embedder",
    "EmbeddingMismatchError",
    "EmbeddingModelSpec",
    "ExpansionReceipt",
    "IndexMode",
    "InvalidRecordError",
    "InvalidSelectionError",
    "InvalidSummaryError",
    "InvariantViolationError",
    "MaintenanceEstimate",
    "MaintenanceReceipt",
    "MaintenanceUnitPreview",
    "NamespaceConfig",
    "NodeDescriptor",
    "PathCollisionError",
    "PolicyConfig",
    "PolicyViolationError",
    "RecordTokenEstimate",
    "RenderOptions",
    "RenderPreview",
    "RenderedHistory",
    "ReservedKeyCollisionError",
    "SnapshotVersionError",
    "StaleRenderError",
    "StaleSummaryError",
    "StructuralStats",
    "Summarizer",
    "SummaryInput",
    "SummaryReceipt",
    "SummaryResult",
    "UnknownPathError",
    "__version__",
]
