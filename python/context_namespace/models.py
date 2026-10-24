"""Public DTO dataclasses for the context-namespace Python facade.

All DTOs are frozen, slotted dataclasses so the wheel has no Pydantic
runtime dependency. Every DTO constructor performs cheap local
type/range checks for fast feedback. Rust repeats authoritative
validation before state mutation.

Important Classes:
    ChildDescriptor: Descriptor for an immediate child node of a summary.
    SummaryInput: Input sent to an external summarizer.
    SummaryResult: Result returned by an external summarizer.
    AppendReceipt: Confirmation of a successful append operation.
    SummaryReceipt: Confirmation of a successful summary commit.
    ExpansionReceipt: Confirmation of a successful expand or collapse.
    RenderedHistory: Result of a successful render operation.
    MaintenanceUnitPreview: Per-record unit for maintenance planning.
    RenderPreview: Preview of a render including maintenance metadata.
    RecordTokenEstimate: Per-record token estimate for maintenance.
    MaintenanceEstimate: Token estimates supplied by the harness.
    MaintenanceReceipt: Result of a maintenance operation.
    NodeDescriptor: Public descriptor for a summary node.
    StructuralStats: Snapshot of structural counts at a point in time.
    Capabilities: Reports library version, schema versions, and features.
    EmbeddingModelSpec: Specifies the embedding model and vector dimensions.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TypeAlias

JsonScalar: TypeAlias = str | int | float | bool | None
JsonValue: TypeAlias = JsonScalar | list["JsonValue"] | dict[str, "JsonValue"]


@dataclass(frozen=True, slots=True)
class ChildDescriptor:
    """Descriptor for an immediate child node of a new summary.

    Attributes:
        node_id: The immutable ID of the child node.
        label: The child node's label.
        gloss: The child node's gloss text.
    """

    node_id: str
    label: str
    gloss: str


@dataclass(frozen=True, slots=True)
class SummaryInput:
    """The input sent to an external summarizer.

    Attributes:
        request_id: The request ID associated with this summary.
        source: The rendered source text extracted from canonical state.
        children: Descriptors for immediate child nodes.
        max_summary_chars: Policy-enforced max character length for the
            summary.
        max_gloss_chars: Policy-enforced max character length for the gloss.
    """

    request_id: str
    source: str
    children: tuple[ChildDescriptor, ...]
    max_summary_chars: int
    max_gloss_chars: int


@dataclass(frozen=True, slots=True)
class SummaryResult:
    """The result returned by an external summarizer.

    Attributes:
        summary: The summary text produced by the summarizer.
        gloss: The short gloss text for collapsed rendering.
    """

    summary: str
    gloss: str


@dataclass(frozen=True, slots=True)
class AppendReceipt:
    """Confirmation of a successful append operation.

    Attributes:
        record_id: The stable ID assigned to the inserted record.
        visible_index: The zero-based visible index of the record.
    """

    record_id: str
    visible_index: int


@dataclass(frozen=True, slots=True)
class SummaryReceipt:
    """Confirmation of a successful summary commit.

    Attributes:
        node_id: The ID of the newly created summary node.
        carrier_record_id: The ID of the carrier record (record-range) or
            edited record (text-slice).
        replaced_record_ids: IDs of the canonical records replaced.
        render_id: The render ID associated with the commit.
    """

    node_id: str
    carrier_record_id: str
    replaced_record_ids: tuple[str, ...]
    render_id: str


@dataclass(frozen=True, slots=True)
class ExpansionReceipt:
    """Confirmation of a successful expand or collapse operation.

    Attributes:
        node_id: The ID of the expanded or collapsed node.
        ttl_renders: The effective TTL in renders after the operation.
        render_id: The render ID associated with the activation.
    """

    node_id: str
    ttl_renders: int
    render_id: str


@dataclass(frozen=True, slots=True)
class RenderedHistory:
    """Result of a successful render operation.

    Attributes:
        render_id: The unique ID for this render.
        namespace_version: The namespace version at render time.
        json: The rendered JSON string.
        bytes: The byte count of ``json``.
        entries: The number of visible entries in the rendered output.
    """

    render_id: str
    namespace_version: int
    json: str
    bytes: int
    entries: int


@dataclass(frozen=True, slots=True)
class MaintenanceUnitPreview:
    """A per-record unit for maintenance planning.

    Attributes:
        record_id: The stable record ID.
        render_digest: SHA-256 digest of the rendered JSON fragment.
        json_fragment: The rendered JSON fragment for this record.
    """

    record_id: str
    render_digest: str
    json_fragment: str


@dataclass(frozen=True, slots=True)
class RenderPreview:
    """A preview of a render including maintenance metadata.

    Unlike RenderedHistory, a preview does not allocate a render_id,
    register visible-index resolutions, emit a render event, or
    decrement expansion TTLs.

    Attributes:
        namespace_version: The namespace version at preview time.
        json: The rendered JSON string.
        bytes: The byte count of ``json``.
        entries: The number of visible entries.
        maintenance_units: Per-record maintenance units for token
            estimation.
    """

    namespace_version: int
    json: str
    bytes: int
    entries: int
    maintenance_units: tuple[MaintenanceUnitPreview, ...]


@dataclass(frozen=True, slots=True)
class RecordTokenEstimate:
    """A per-record token estimate bound to a stable record ID and digest.

    Attributes:
        record_id: The stable record ID this estimate applies to.
        render_digest: SHA-256 digest of the record's rendered fragment.
        estimated_tokens: The harness-supplied token estimate.
    """

    record_id: str
    render_digest: str
    estimated_tokens: int


@dataclass(frozen=True, slots=True)
class MaintenanceEstimate:
    """Token estimates supplied by the harness before maintenance.

    Attributes:
        estimated_next_tokens: Harness estimate of total tokens in the
            next render.
        records: Per-record token estimates with digests.
    """

    estimated_next_tokens: int
    records: tuple[RecordTokenEstimate, ...]


@dataclass(frozen=True, slots=True)
class MaintenanceReceipt:
    """Result of a maintenance operation.

    Attributes:
        action: Either ``"no_action"`` or ``"summarized"``.
        receipt: The SummaryReceipt if action is ``"summarized"``, else
            None.
    """

    action: str
    receipt: SummaryReceipt | None = None

    @classmethod
    def no_action(cls) -> MaintenanceReceipt:
        """Creates a no-action receipt.

        Returns:
            A MaintenanceReceipt with action set to ``"no_action"``.
        """
        return cls(action="no_action")

    @classmethod
    def summarized(cls, receipt: SummaryReceipt) -> MaintenanceReceipt:
        """Creates a summarized receipt.

        Args:
            receipt: The SummaryReceipt from the committed maintenance
                summary.

        Returns:
            A MaintenanceReceipt with action set to ``"summarized"``.
        """
        return cls(action="summarized", receipt=receipt)


@dataclass(frozen=True, slots=True)
class NodeDescriptor:
    """Public descriptor for a summary node.

    Attributes:
        node_id: The immutable node ID.
        path: The current absolute path of the node.
        label: The node's label.
        parent_id: The parent node ID, or None for a root.
        child_count: The number of immediate children.
        summary_length: The character length of the summary text.
        summary: The summary text.
        gloss: The gloss text.
        expanded: Whether the node is currently expanded.
        source_kind: Either ``"TextSlice"`` or ``"RecordRange"``.
    """

    node_id: str
    path: str
    label: str
    parent_id: str | None
    child_count: int
    summary_length: int
    summary: str
    gloss: str
    expanded: bool
    source_kind: str


@dataclass(frozen=True, slots=True)
class StructuralStats:
    """Snapshot of structural counts at a point in time.

    Attributes:
        records: Number of canonical records.
        nodes: Number of summary nodes.
        active_expansions: Number of active expansions (TTL > 0).
        pending_requests: Number of pending summary requests.
        alias_count: Number of retained path aliases.
        max_depth: Maximum tree depth (0 = no nodes).
        dropped_events: Events dropped due to queue overflow.
    """

    records: int
    nodes: int
    active_expansions: int
    pending_requests: int
    alias_count: int
    max_depth: int
    dropped_events: int


@dataclass(frozen=True, slots=True)
class Capabilities:
    """Reports library version, schema versions, and enabled features.

    Attributes:
        library_version: The crate version from Cargo.toml.
        rust_api_version: The high-level Rust API version.
        snapshot_schema: Snapshot schema version.
        render_schema: Render schema version.
        policy_schema: Policy schema version.
        features: Enabled cargo feature names.
    """

    library_version: str
    rust_api_version: str
    snapshot_schema: int
    render_schema: int
    policy_schema: int
    features: frozenset[str]


@dataclass(frozen=True, slots=True)
class EmbeddingModelSpec:
    """Specifies the embedding model and vector dimensions.

    Attributes:
        model: The model identifier string.
        dimension: The number of dimensions in the embedding vector.
    """

    model: str
    dimension: int
