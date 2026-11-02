"""Python facade for the context-namespace Rust library.

This module provides the public ContextNamespace class that wraps the
native extension module with ergonomic methods, dataclass DTOs, and
error mapping. Callers should never import from _native directly.

Important Classes:
    ContextNamespace: Public facade wrapping _NativeNamespace.

Important Functions:
    _map_native_errors: Decorator that catches native exceptions and
        re-raises them as Python exceptions.
    _to_dataclass: Converts a dataclass instance to a plain dict.
"""

from __future__ import annotations

import dataclasses
from collections.abc import Mapping, Sequence

from ._native import (
    ContextNamespaceError as _NativeContextNamespaceError,
)
from ._native import (
    EmbeddingMismatchError as _NativeEmbeddingMismatchError,
)
from ._native import (
    InvalidRecordError as _NativeInvalidRecordError,
)
from ._native import (
    InvalidSelectionError as _NativeInvalidSelectionError,
)
from ._native import (
    InvalidSummaryError as _NativeInvalidSummaryError,
)
from ._native import (
    InvariantViolationError as _NativeInvariantViolationError,
)
from ._native import (
    PathCollisionError as _NativePathCollisionError,
)
from ._native import (
    PolicyViolationError as _NativePolicyViolationError,
)
from ._native import (
    SnapshotVersionError as _NativeSnapshotVersionError,
)
from ._native import (
    StaleRenderError as _NativeStaleRenderError,
)
from ._native import (
    StaleSummaryError as _NativeStaleSummaryError,
)
from ._native import (
    UnknownPathError as _NativeUnknownPathError,
)
from ._native import _NativeNamespace, _PyPreparedSummary
from .config import NamespaceConfig
from .errors import (
    ContextNamespaceError,
    EmbeddingMismatchError,
    InvalidRecordError,
    InvalidSelectionError,
    InvalidSummaryError,
    InvariantViolationError,
    PathCollisionError,
    PolicyViolationError,
    SnapshotVersionError,
    StaleRenderError,
    StaleSummaryError,
    UnknownPathError,
)
from .models import (
    AppendReceipt,
    Capabilities,
    ExpansionReceipt,
    MaintenanceEstimate,
    MaintenanceReceipt,
    MaintenanceUnitPreview,
    NodeDescriptor,
    RenderedHistory,
    RenderPreview,
    StructuralStats,
    SummaryInput,
    SummaryReceipt,
    SummaryResult,
)
from .protocols import Summarizer

JsonValue = str | int | float | bool | None | list["JsonValue"] | dict[str, "JsonValue"]

_NATIVE_TO_PY = {
    _NativeContextNamespaceError: ContextNamespaceError,
    _NativeInvalidRecordError: InvalidRecordError,
    _NativeInvalidSelectionError: InvalidSelectionError,
    _NativeStaleRenderError: StaleRenderError,
    _NativeUnknownPathError: UnknownPathError,
    _NativePathCollisionError: PathCollisionError,
    _NativePolicyViolationError: PolicyViolationError,
    _NativeStaleSummaryError: StaleSummaryError,
    _NativeInvalidSummaryError: InvalidSummaryError,
    _NativeEmbeddingMismatchError: EmbeddingMismatchError,
    _NativeSnapshotVersionError: SnapshotVersionError,
    _NativeInvariantViolationError: InvariantViolationError,
}


def _map_native_errors(func):
    """Decorator that catches native exceptions and re-raises as Python exceptions.

    Iterates the native-to-Python mapping in reverse order so that
    subclass-specific exceptions are matched before their base classes.
    Works with both synchronous and asynchronous functions.

    Args:
        func: The function to wrap.

    Returns:
        The wrapped function with native error mapping.
    """
    import functools
    import inspect

    if inspect.iscoroutinefunction(func):

        @functools.wraps(func)
        async def async_wrapper(*args, **kwargs):
            try:
                return await func(*args, **kwargs)
            except Exception as e:
                for native_cls, py_cls in reversed(list(_NATIVE_TO_PY.items())):
                    if isinstance(e, native_cls):
                        raise py_cls(str(e)) from e
                raise

        return async_wrapper

    @functools.wraps(func)
    def wrapper(*args, **kwargs):
        try:
            return func(*args, **kwargs)
        except Exception as e:
            for native_cls, py_cls in reversed(list(_NATIVE_TO_PY.items())):
                if isinstance(e, native_cls):
                    raise py_cls(str(e)) from e
            raise

    return wrapper


def _to_dataclass(obj: object) -> dict[str, object]:
    """Converts a dataclass instance or dict to a plain dict.

    Args:
        obj: A dataclass instance or a dict.

    Returns:
        The input as a plain dict.

    Raises:
        TypeError: If the input is neither a dataclass instance nor a dict.
    """
    if isinstance(obj, dict):
        return obj
    if dataclasses.is_dataclass(obj) and not isinstance(obj, type):
        return dataclasses.asdict(obj)  # type: ignore[arg-type]
    raise TypeError(f"expected dict or dataclass, got {type(obj).__name__}")


class ContextNamespace:
    """Public facade wrapping _NativeNamespace with ergonomic methods.

    This class provides a thin, stdlib-only Python layer over the native
    extension module. It converts native return values to frozen
    dataclass DTOs and maps native exceptions to the Python exception
    hierarchy defined in errors.py.

    Instance Attributes:
        _native: The underlying _NativeNamespace instance.
    """

    def __init__(
        self,
        *,
        config: NamespaceConfig,
        records: Sequence[Mapping[str, JsonValue]] = (),
    ) -> None:
        """Initializes the namespace with a config and optional records.

        Args:
            config: The namespace configuration.
            records: Optional initial records to append.
        """
        self._native = _NativeNamespace(
            _to_dataclass(config),
            [dict(r) for r in records],
        )

    @_map_native_errors
    def append(self, record: Mapping[str, JsonValue]) -> AppendReceipt:
        """Appends a record to the canonical sequence.

        Args:
            record: A mapping with at least the configured render field
                as a string.

        Returns:
            An AppendReceipt with the stable record ID and visible index.
        """
        r = self._native.append(dict(record))
        return AppendReceipt(record_id=r.record_id, visible_index=r.visible_index)

    @_map_native_errors
    def render(self) -> RenderedHistory:
        """Renders the current namespace state as deterministic JSON.

        Returns:
            A RenderedHistory containing the render ID, JSON string,
            byte count, and entry count.
        """
        r = self._native.render()
        return RenderedHistory(
            render_id=r.render_id,
            namespace_version=r.namespace_version,
            json=r.json,
            bytes=r.bytes,
            entries=r.entries,
        )

    @_map_native_errors
    def preview_render(self) -> RenderPreview:
        """Produces a non-mutating render preview with maintenance metadata.

        Unlike render, this does not allocate a render_id, register
        resolutions, emit an event, or decrement TTLs.

        Returns:
            A RenderPreview with per-record maintenance units.
        """
        p = self._native._preview_render()
        units = tuple(
            MaintenanceUnitPreview(
                record_id=u.record_id,
                render_digest=u.render_digest,
                json_fragment=u.json_fragment,
            )
            for u in p.maintenance_units
        )
        return RenderPreview(
            namespace_version=p.namespace_version,
            json=p.json,
            bytes=p.bytes,
            entries=p.entries,
            maintenance_units=units,
        )

    @_map_native_errors
    async def summarize_records(
        self,
        *,
        render_id: str,
        start_index: int,
        end_index: int,
        label: str,
        summarizer: Summarizer,
    ) -> SummaryReceipt:
        """Summarizes a consecutive record range in one awaitable call.

        Prepares a record-range summary, calls the summarizer, and
        commits the result. If the summarizer raises, the pending
        request is cancelled and state is not mutated.

        Args:
            render_id: The render ID from a recent render call.
            start_index: The inclusive start visible index.
            end_index: The exclusive end visible index.
            label: The path label for the new summary node.
            summarizer: An implementation of the Summarizer protocol.

        Returns:
            A SummaryReceipt with the new node ID and replaced records.
        """
        prepared = self._native._prepare_records(
            render_id, start_index, end_index, label
        )
        inp = SummaryInput(
            request_id=prepared.request_id,
            source=prepared.source,
            children=(),
            max_summary_chars=prepared.max_summary_chars,
            max_gloss_chars=prepared.max_gloss_chars,
        )
        try:
            result = await summarizer.summarize(inp)
        except BaseException:
            self._native._cancel(prepared)
            raise
        return self._commit_prepared(prepared, result)

    @_map_native_errors
    async def summarize_text(
        self,
        *,
        render_id: str,
        record_index: int,
        start_char: int,
        end_char: int,
        label: str,
        summarizer: Summarizer,
    ) -> SummaryReceipt:
        """Summarizes a character range within one record's render field.

        Prepares a text-slice summary, calls the summarizer, and commits
        the result. If the summarizer raises, the pending request is
        cancelled and state is not mutated.

        Args:
            render_id: The render ID from a recent render call.
            record_index: The visible index of the target record.
            start_char: Unicode scalar-value offset (inclusive).
            end_char: Unicode scalar-value offset (exclusive).
            label: The path label for the new summary node.
            summarizer: An implementation of the Summarizer protocol.

        Returns:
            A SummaryReceipt with the new node ID.
        """
        prepared = self._native._prepare_text(
            render_id, record_index, start_char, end_char, label
        )
        inp = SummaryInput(
            request_id=prepared.request_id,
            source=prepared.source,
            children=(),
            max_summary_chars=prepared.max_summary_chars,
            max_gloss_chars=prepared.max_gloss_chars,
        )
        try:
            result = await summarizer.summarize(inp)
        except BaseException:
            self._native._cancel(prepared)
            raise
        return self._commit_prepared(prepared, result)

    @_map_native_errors
    async def maintain(
        self,
        *,
        estimate: MaintenanceEstimate,
        summarizer: Summarizer,
    ) -> MaintenanceReceipt:
        """Runs automatic maintenance based on harness-supplied token estimates.

        Plans maintenance, optionally calls the summarizer, and commits
        if triggered. If the policy decides no action is needed, returns
        a no-action receipt without calling the summarizer.

        Args:
            estimate: Per-record token estimates with digests.
            summarizer: An implementation of the Summarizer protocol.

        Returns:
            A MaintenanceReceipt indicating no-action or summarized.
        """
        native_estimate = _NativeEstimate(
            estimated_next_tokens=estimate.estimated_next_tokens,
            records=[
                _NativeRecordTokenEstimate(
                    record_id=r.record_id,
                    render_digest=r.render_digest,
                    estimated_tokens=r.estimated_tokens,
                )
                for r in estimate.records
            ],
        )
        prepared = self._native._plan_maintenance(native_estimate)
        if prepared is None:
            return MaintenanceReceipt.no_action()
        inp = SummaryInput(
            request_id=prepared.request_id,
            source=prepared.source,
            children=(),
            max_summary_chars=prepared.max_summary_chars,
            max_gloss_chars=prepared.max_gloss_chars,
        )
        try:
            result = await summarizer.summarize(inp)
        except BaseException:
            self._native._cancel(prepared)
            raise
        receipt = self._commit_prepared(prepared, result)
        return MaintenanceReceipt.summarized(receipt)

    def _commit_prepared(
        self, prepared: _PyPreparedSummary, result: SummaryResult
    ) -> SummaryReceipt:
        """Commits a summarizer result and releases the pending request on any failure."""
        try:
            summary, gloss = result.summary, result.gloss
            r = self._native._commit_summary(prepared, summary, gloss)
        except BaseException:
            try:
                self._native._cancel(prepared)
            except Exception:
                pass
            raise
        return SummaryReceipt(
            node_id=r.node_id,
            carrier_record_id=r.carrier_record_id,
            replaced_record_ids=tuple(r.replaced_record_ids),
            render_id=r.render_id,
        )

    @_map_native_errors
    def expand(self, path: str, *, ttl_renders: int = 2) -> ExpansionReceipt:
        """Expands a summary node for the next ``ttl_renders`` renders.

        Args:
            path: The absolute path of the node (e.g. ``"/batch-1"``).
            ttl_renders: The number of renders to keep the node expanded.

        Returns:
            An ExpansionReceipt with the effective TTL.
        """
        r = self._native.expand(path, ttl_renders)
        return ExpansionReceipt(
            node_id=r.node_id, ttl_renders=r.ttl_renders, render_id=r.render_id
        )

    @_map_native_errors
    def collapse(self, path: str) -> ExpansionReceipt:
        """Collapses a summary node immediately.

        Canonical state is not modified; only the expansion overlay is
        cleared.

        Args:
            path: The absolute path of the node to collapse.

        Returns:
            An ExpansionReceipt with ttl_renders set to 0.
        """
        r = self._native.collapse(path)
        return ExpansionReceipt(
            node_id=r.node_id, ttl_renders=r.ttl_renders, render_id=r.render_id
        )

    @_map_native_errors
    def is_expanded(self, path: str) -> bool:
        """Checks whether a node at the given path is currently expanded.

        Args:
            path: The absolute path of the node.

        Returns:
            True if the node has an active expansion, False otherwise.
        """
        return self._native.is_expanded(path)

    @_map_native_errors
    def list_nodes(self) -> list[NodeDescriptor]:
        """Returns descriptors for all nodes in the tree.

        Returns:
            A list of NodeDescriptor instances.
        """
        nodes = self._native.list_nodes()
        return [
            NodeDescriptor(
                node_id=n.node_id,
                path=n.path,
                label=n.label,
                parent_id=n.parent_id,
                child_count=n.child_count,
                summary_length=n.summary_length,
                summary=n.summary,
                gloss=n.gloss,
                expanded=n.expanded,
                source_kind=n.source_kind,
            )
            for n in nodes
        ]

    @_map_native_errors
    def stats(self) -> StructuralStats:
        """Returns structural statistics for the namespace.

        Returns:
            A StructuralStats with record/node counts, depth, and more.
        """
        s = self._native.stats()
        return StructuralStats(
            records=s.records,
            nodes=s.nodes,
            active_expansions=s.active_expansions,
            pending_requests=s.pending_requests,
            alias_count=s.alias_count,
            max_depth=s.max_depth,
            dropped_events=s.dropped_events,
        )

    def capabilities(self) -> Capabilities:
        """Returns enabled features and schema versions.

        Returns:
            A Capabilities instance with version and feature info.
        """
        c = self._native.capabilities()
        return Capabilities(
            library_version=c.library_version,
            rust_api_version=c.rust_api_version,
            snapshot_schema=c.snapshot_schema,
            render_schema=c.render_schema,
            policy_schema=c.policy_schema,
            features=frozenset(c.features),
        )

    @_map_native_errors
    def validate(self) -> None:
        """Validates all namespace invariants.

        Raises:
            InvariantViolationError: If any invariant is violated.
        """
        self._native.validate()

    @_map_native_errors
    def snapshot(self) -> bytes:
        """Serializes the namespace to versioned JSON with a content hash.

        Returns:
            The snapshot bytes.
        """
        return bytes(self._native.snapshot())

    @classmethod
    @_map_native_errors
    def restore(
        cls, data: bytes, *, config: NamespaceConfig | None = None
    ) -> ContextNamespace:
        """Rebuilds a namespace from snapshot bytes.

        Args:
            data: The snapshot bytes from a previous snapshot call.
            config: Unused; retained for API compatibility.

        Returns:
            A new ContextNamespace restored from the snapshot.
        """
        native = _NativeNamespace.restore(data)
        ns = cls.__new__(cls)
        ns._native = native
        return ns

    @property
    def version(self) -> int:
        """int: The current namespace version."""
        return self._native.version()

    @_map_native_errors
    def drain_events(self, limit: int = 100) -> list[str]:
        """Drains up to ``limit`` events from the internal event queue.

        Args:
            limit: The maximum number of events to drain.

        Returns:
            A list of event string representations.
        """
        return self._native.drain_events(limit)


class _NativeEstimate:
    """Internal helper for building native maintenance estimate objects.

    Instance Attributes:
        estimated_next_tokens: Total token estimate for the next render.
        records: Per-record token estimates.
    """

    def __init__(
        self,
        *,
        estimated_next_tokens: int,
        records: list[_NativeRecordTokenEstimate],
    ) -> None:
        self.estimated_next_tokens = estimated_next_tokens
        self.records = records


class _NativeRecordTokenEstimate:
    """Internal helper for building native per-record token estimates.

    Instance Attributes:
        record_id: The stable record ID.
        render_digest: SHA-256 digest of the rendered fragment.
        estimated_tokens: The harness-supplied token estimate.
    """

    def __init__(
        self,
        *,
        record_id: str,
        render_digest: str,
        estimated_tokens: int,
    ) -> None:
        self.record_id = record_id
        self.render_digest = render_digest
        self.estimated_tokens = estimated_tokens
