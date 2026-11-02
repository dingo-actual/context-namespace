"""Shared fixtures and helpers for the context-namespace Python test suite.

Important Classes:
    StubSummarizer: A deterministic summarizer for testing.
    CountingSummarizer: A summarizer that counts calls.

Important Functions:
    make_config: Returns a default NamespaceConfig.
    make_tight_config: Returns a config with small defensive limits.
    make_record: Returns a record dict with the given content and role.
    append_records: Appends N records to a namespace and returns it.
    summarize_records_sync: Runs summarize_records synchronously via asyncio.run.
    summarize_text_sync: Runs summarize_text synchronously via asyncio.run.
    maintain_sync: Runs maintain synchronously via asyncio.run.
"""

from __future__ import annotations

import asyncio

import pytest
from context_namespace import (
    ContextNamespace,
    MaintenanceEstimate,
    MaintenanceReceipt,
    NamespaceConfig,
    RecordTokenEstimate,
    SummaryReceipt,
    SummaryResult,
)
from context_namespace.protocols import Summarizer


class StubSummarizer:
    """A deterministic summarizer for testing.

    Returns a fixed summary and gloss for any input.
    """

    async def summarize(self, request) -> SummaryResult:
        return SummaryResult(
            summary=f"Summary of {len(request.source)} chars",
            gloss="test-gloss",
        )


class CountingSummarizer:
    """A summarizer that counts how many times summarize was called.

    Instance Attributes:
        call_count: The number of times summarize was called.
    """

    def __init__(self) -> None:
        self.call_count: int = 0

    async def summarize(self, request) -> SummaryResult:
        self.call_count += 1
        return SummaryResult(
            summary=f"Summary of {len(request.source)} chars",
            gloss="test-gloss",
        )


class FailingSummarizer:
    """A summarizer that always raises an error.

    Instance Attributes:
        error: The exception to raise.
    """

    def __init__(self, error: Exception) -> None:
        self.error = error

    async def summarize(self, request) -> SummaryResult:
        raise self.error


def make_config() -> NamespaceConfig:
    """Returns a default NamespaceConfig.

    Returns:
        A NamespaceConfig with default settings.
    """
    return NamespaceConfig()


def make_tight_config(
    *,
    max_records: int = 3,
    max_nodes: int = 5,
    max_event_queue: int = 8,
) -> NamespaceConfig:
    """Returns a config with small defensive limits for boundary testing.

    Args:
        max_records: Maximum number of canonical records.
        max_nodes: Maximum number of summary nodes.
        max_event_queue: Maximum number of events in the queue.

    Returns:
        A NamespaceConfig with the given tight limits.
    """
    from context_namespace import DefensiveLimits

    return NamespaceConfig(
        limits=DefensiveLimits(
            max_records=max_records,
            max_nodes=max_nodes,
            max_event_queue=max_event_queue,
        ),
    )


def make_record(content: str, role: str = "user") -> dict[str, str]:
    """Returns a record dict with the given content and role.

    Args:
        content: The content string for the record.
        role: The role string for the record.

    Returns:
        A dict with 'role' and 'content' keys.
    """
    return {"role": role, "content": content}


def append_records(ns: ContextNamespace, count: int, prefix: str = "record") -> ContextNamespace:
    """Appends N records to a namespace and returns it.

    Args:
        ns: The namespace to append to.
        count: The number of records to append.
        prefix: The prefix for record content strings.

    Returns:
        The same namespace, for chaining.
    """
    for i in range(count):
        ns.append(make_record(f"{prefix} {i}"))
    return ns


def summarize_records_sync(
    ns: ContextNamespace,
    *,
    render_id: str,
    start_index: int,
    end_index: int,
    label: str,
    summarizer: Summarizer | None = None,
) -> SummaryReceipt:
    """Runs summarize_records synchronously via asyncio.run.

    Args:
        ns: The namespace to operate on.
        render_id: The render ID from a recent render call.
        start_index: The inclusive start visible index.
        end_index: The exclusive end visible index.
        label: The path label for the new summary node.
        summarizer: A Summarizer implementation. Defaults to StubSummarizer.

    Returns:
        The SummaryReceipt from the summarize operation.
    """
    if summarizer is None:
        summarizer = StubSummarizer()
    return asyncio.run(
        ns.summarize_records(
            render_id=render_id,
            start_index=start_index,
            end_index=end_index,
            label=label,
            summarizer=summarizer,
        )
    )


def summarize_text_sync(
    ns: ContextNamespace,
    *,
    render_id: str,
    record_index: int,
    start_char: int,
    end_char: int,
    label: str,
    summarizer: Summarizer | None = None,
) -> SummaryReceipt:
    """Runs summarize_text synchronously via asyncio.run.

    Args:
        ns: The namespace to operate on.
        render_id: The render ID from a recent render call.
        record_index: The visible index of the target record.
        start_char: Unicode scalar-value offset (inclusive).
        end_char: Unicode scalar-value offset (exclusive).
        label: The path label for the new summary node.
        summarizer: A Summarizer implementation. Defaults to StubSummarizer.

    Returns:
        The SummaryReceipt from the summarize operation.
    """
    if summarizer is None:
        summarizer = StubSummarizer()
    return asyncio.run(
        ns.summarize_text(
            render_id=render_id,
            record_index=record_index,
            start_char=start_char,
            end_char=end_char,
            label=label,
            summarizer=summarizer,
        )
    )


def maintain_sync(
    ns: ContextNamespace,
    *,
    estimated_next_tokens: int,
    per_record_tokens: int = 500,
    summarizer: Summarizer | None = None,
) -> MaintenanceReceipt:
    """Runs maintain synchronously via asyncio.run.

    Args:
        ns: The namespace to operate on.
        estimated_next_tokens: Total token estimate for the next render.
        per_record_tokens: Token estimate per record.
        summarizer: A Summarizer implementation. Defaults to StubSummarizer.

    Returns:
        The MaintenanceReceipt from the maintain operation.
    """
    if summarizer is None:
        summarizer = StubSummarizer()
    preview = ns.preview_render()
    estimate = MaintenanceEstimate(
        estimated_next_tokens=estimated_next_tokens,
        records=tuple(
            RecordTokenEstimate(
                record_id=u.record_id,
                render_digest=u.render_digest,
                estimated_tokens=per_record_tokens,
            )
            for u in preview.maintenance_units
        ),
    )
    return asyncio.run(ns.maintain(estimate=estimate, summarizer=summarizer))


def build_estimate(
    ns: ContextNamespace,
    *,
    estimated_next_tokens: int,
    per_record_tokens: int = 500,
) -> MaintenanceEstimate:
    """Builds a MaintenanceEstimate from a preview_render of the namespace.

    Args:
        ns: The namespace to preview.
        estimated_next_tokens: Total token estimate for the next render.
        per_record_tokens: Token estimate per record.

    Returns:
        A MaintenanceEstimate with per-record estimates matching the preview.
    """
    preview = ns.preview_render()
    return MaintenanceEstimate(
        estimated_next_tokens=estimated_next_tokens,
        records=tuple(
            RecordTokenEstimate(
                record_id=u.record_id,
                render_digest=u.render_digest,
                estimated_tokens=per_record_tokens,
            )
            for u in preview.maintenance_units
        ),
    )


@pytest.fixture
def ns() -> ContextNamespace:
    """Returns a fresh empty ContextNamespace with default config.

    Yields:
        A ContextNamespace instance.
    """
    return ContextNamespace(config=make_config())


@pytest.fixture
def ns_with_records(ns: ContextNamespace) -> ContextNamespace:
    """Returns a ContextNamespace with 4 records appended.

    Yields:
        A ContextNamespace with 4 records.
    """
    return append_records(ns, 4)
