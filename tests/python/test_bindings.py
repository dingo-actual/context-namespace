import pytest
from context_namespace import (
    ContextNamespace,
    InvalidRecordError,
    InvalidSelectionError,
    NamespaceConfig,
    UnknownPathError,
)


class StubSummarizer:
    """A deterministic summarizer for testing."""

    async def summarize(self, request):
        from context_namespace import SummaryResult

        return SummaryResult(
            summary=f"Summary of {len(request.source)} chars",
            gloss="test-gloss",
        )


def make_config():
    return NamespaceConfig()


def make_record(content: str, role: str = "user"):
    return {"role": role, "content": content}


def test_append_and_stats():
    ns = ContextNamespace(config=make_config())
    assert ns.stats().records == 0
    assert ns.stats().nodes == 0
    ns.append(make_record("hello"))
    ns.append(make_record("world"))
    assert ns.stats().records == 2
    assert ns.version == 2


def test_render():
    ns = ContextNamespace(config=make_config())
    ns.append(make_record("hello"))
    ns.append(make_record("world"))
    view = ns.render()
    assert view.entries == 2
    assert "hello" in view.json
    assert "world" in view.json
    assert view.namespace_version == ns.version


def test_append_invalid_record():
    ns = ContextNamespace(config=make_config())
    with pytest.raises(InvalidRecordError):
        ns.append({"role": "user"})


def test_summarize_records():
    ns = ContextNamespace(config=make_config())
    for i in range(5):
        ns.append(make_record(f"record {i}"))
    view = ns.render()
    import asyncio

    receipt = asyncio.run(
        ns.summarize_records(
            render_id=view.render_id,
            start_index=0,
            end_index=3,
            label="batch-1",
            summarizer=StubSummarizer(),
        )
    )
    assert receipt.node_id
    assert ns.stats().nodes == 1
    assert ns.stats().records == 3


def test_summarize_stale_render():
    ns = ContextNamespace(config=make_config())
    ns.append(make_record("a"))
    view = ns.render()
    ns.append(make_record("b"))
    import asyncio

    with pytest.raises((InvalidSelectionError, Exception)):
        asyncio.run(
            ns.summarize_records(
                render_id=view.render_id,
                start_index=0,
                end_index=1,
                label="bad",
                summarizer=StubSummarizer(),
            )
        )


def test_expand_and_collapse():
    ns = ContextNamespace(config=make_config())
    for i in range(4):
        ns.append(make_record(f"record {i}"))
    import asyncio

    view = ns.render()
    asyncio.run(
        ns.summarize_records(
            render_id=view.render_id,
            start_index=0,
            end_index=2,
            label="batch",
            summarizer=StubSummarizer(),
        )
    )
    assert ns.is_expanded("/batch") is False
    ns.expand("/batch", ttl_renders=3)
    assert ns.is_expanded("/batch") is True
    ns.collapse("/batch")
    assert ns.is_expanded("/batch") is False


def test_expand_unknown_path():
    ns = ContextNamespace(config=make_config())
    with pytest.raises(UnknownPathError):
        ns.expand("/nonexistent", ttl_renders=1)


def test_list_nodes():
    ns = ContextNamespace(config=make_config())
    for i in range(4):
        ns.append(make_record(f"record {i}"))
    import asyncio

    view = ns.render()
    asyncio.run(
        ns.summarize_records(
            render_id=view.render_id,
            start_index=0,
            end_index=2,
            label="batch",
            summarizer=StubSummarizer(),
        )
    )
    nodes = ns.list_nodes()
    assert len(nodes) == 1
    assert nodes[0].label == "batch"
    assert nodes[0].path == "/batch"
    assert nodes[0].expanded is False


def test_snapshot_and_restore():
    ns = ContextNamespace(config=make_config())
    for i in range(3):
        ns.append(make_record(f"record {i}"))
    import asyncio

    view = ns.render()
    asyncio.run(
        ns.summarize_records(
            render_id=view.render_id,
            start_index=0,
            end_index=2,
            label="batch",
            summarizer=StubSummarizer(),
        )
    )
    snap = ns.snapshot()
    assert len(snap) > 0

    restored = ContextNamespace.restore(snap)
    assert restored.stats().nodes == 1
    assert restored.stats().records == 2
    assert restored.version == ns.version


def test_capabilities():
    ns = ContextNamespace(config=make_config())
    caps = ns.capabilities()
    assert caps.library_version
    assert caps.snapshot_schema > 0
    assert caps.render_schema > 0
    assert isinstance(caps.features, frozenset)


def test_validate():
    ns = ContextNamespace(config=make_config())
    for i in range(3):
        ns.append(make_record(f"record {i}"))
    ns.validate()


def test_maintain_no_action():
    ns = ContextNamespace(config=make_config())
    for i in range(3):
        ns.append(make_record(f"record {i}"))
    preview = ns.preview_render()
    from context_namespace import (
        MaintenanceEstimate,
        RecordTokenEstimate,
    )

    estimate = MaintenanceEstimate(
        estimated_next_tokens=100,
        records=tuple(
            RecordTokenEstimate(
                record_id=u.record_id,
                render_digest=u.render_digest,
                estimated_tokens=50,
            )
            for u in preview.maintenance_units
        ),
    )
    import asyncio

    result = asyncio.run(ns.maintain(estimate=estimate, summarizer=StubSummarizer()))
    assert result.action == "no_action"
    assert result.receipt is None


def test_maintain_summarize():
    ns = ContextNamespace(config=make_config())
    for i in range(12):
        ns.append(make_record(f"record {i} with some content here"))
    preview = ns.preview_render()
    from context_namespace import (
        MaintenanceEstimate,
        RecordTokenEstimate,
    )

    estimate = MaintenanceEstimate(
        estimated_next_tokens=50_000,
        records=tuple(
            RecordTokenEstimate(
                record_id=u.record_id,
                render_digest=u.render_digest,
                estimated_tokens=5_000,
            )
            for u in preview.maintenance_units
        ),
    )
    import asyncio

    result = asyncio.run(ns.maintain(estimate=estimate, summarizer=StubSummarizer()))
    assert result.action == "summarized"
    assert result.receipt is not None


def test_drain_events():
    ns = ContextNamespace(config=make_config())
    ns.append(make_record("a"))
    events = ns.drain_events()
    assert len(events) > 0


def test_cancel_on_summarizer_error():
    ns = ContextNamespace(config=make_config())
    for i in range(4):
        ns.append(make_record(f"record {i}"))

    class FailingSummarizer:
        async def summarize(self, request):
            raise RuntimeError("inference failed")

    view = ns.render()
    import asyncio

    with pytest.raises(RuntimeError, match="inference failed"):
        asyncio.run(
            ns.summarize_records(
                render_id=view.render_id,
                start_index=0,
                end_index=2,
                label="batch",
                summarizer=FailingSummarizer(),
            )
        )
    assert ns.stats().pending_requests == 0
    assert ns.stats().nodes == 0
