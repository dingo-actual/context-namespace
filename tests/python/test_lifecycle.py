"""End-to-end lifecycle and integration tests for the Python facade.

These tests exercise multi-operation sequences that combine append,
render, summarize, expand, maintenance, snapshot/restore, and
validation into full lifecycle workflows. They verify state and
invariants at each step.
"""

from __future__ import annotations

import pytest
from conftest import (
    CountingSummarizer,
    append_records,
    maintain_sync,
    make_record,
    summarize_records_sync,
    summarize_text_sync,
)
from context_namespace import (
    ChildDescriptor,
    ContextNamespace,
    NamespaceConfig,
    SummaryInput,
    SummaryResult,
)


class TestFullLifecycle:
    """Exercise the full operation lifecycle in a single sequence."""

    def test_append_render_summarize_expand_snapshot_restore_validate(self):
        ns = ContextNamespace(config=NamespaceConfig())
        append_records(ns, 6)
        assert ns.version == 6

        view = ns.render()
        assert view.entries == 6
        assert view.namespace_version == 6

        receipt = summarize_records_sync(
            ns,
            render_id=view.render_id,
            start_index=0,
            end_index=3,
            label="batch-1",
        )
        assert receipt.node_id
        assert ns.stats().records == 4
        assert ns.stats().nodes == 1

        ns.expand("/batch-1", ttl_renders=3)
        assert ns.is_expanded("/batch-1") is True

        snap = ns.snapshot()
        assert len(snap) > 0

        restored = ContextNamespace.restore(snap)
        assert restored.version == ns.version
        assert restored.stats().records == ns.stats().records
        assert restored.stats().nodes == ns.stats().nodes
        assert restored.is_expanded("/batch-1") is True

        restored.validate()

    def test_nested_summaries_create_parent_child_relationship(self):
        ns = ContextNamespace(config=NamespaceConfig())
        append_records(ns, 8)
        view = ns.render()

        summarize_records_sync(
            ns,
            render_id=view.render_id,
            start_index=0,
            end_index=4,
            label="first",
        )
        assert ns.stats().nodes == 1

        view2 = ns.render()
        summarize_records_sync(
            ns,
            render_id=view2.render_id,
            start_index=0,
            end_index=2,
            label="second",
        )
        assert ns.stats().nodes == 2

        nodes = ns.list_nodes()
        paths = {n.path for n in nodes}
        # The second summary encompasses the carrier of the first,
        # so "second" is the parent and "first" is the child.
        assert "/second" in paths
        assert "/second/first" in paths

    def test_version_monotonicity_across_operations(self):
        ns = ContextNamespace(config=NamespaceConfig())
        append_records(ns, 4)
        v_after_append = ns.version
        assert v_after_append == 4

        ns.render()
        assert ns.version == v_after_append

        view = ns.render()
        summarize_records_sync(
            ns,
            render_id=view.render_id,
            start_index=0,
            end_index=2,
            label="batch",
        )
        assert ns.version > v_after_append

        v_after_summary = ns.version
        ns.validate()
        assert ns.version == v_after_summary

        ns.expand("/batch", ttl_renders=2)
        assert ns.version == v_after_summary

        ns.snapshot()
        assert ns.version == v_after_summary


class TestExpansionTtlBehavior:
    """Test expansion TTL decrement and auto-collapse."""

    def test_ttl_decrements_on_each_render(self):
        ns = ContextNamespace(config=NamespaceConfig())
        append_records(ns, 4)
        view = ns.render()
        summarize_records_sync(
            ns,
            render_id=view.render_id,
            start_index=0,
            end_index=2,
            label="batch",
        )
        ns.expand("/batch", ttl_renders=3)
        assert ns.is_expanded("/batch") is True

        ns.render()
        assert ns.is_expanded("/batch") is True

        ns.render()
        assert ns.is_expanded("/batch") is True

        ns.render()
        assert ns.is_expanded("/batch") is False

    def test_ttl_of_one_collapses_after_single_render(self):
        ns = ContextNamespace(config=NamespaceConfig())
        append_records(ns, 4)
        view = ns.render()
        summarize_records_sync(
            ns,
            render_id=view.render_id,
            start_index=0,
            end_index=2,
            label="batch",
        )
        ns.expand("/batch", ttl_renders=1)
        assert ns.is_expanded("/batch") is True
        ns.render()
        assert ns.is_expanded("/batch") is False

    def test_re_expansion_never_shortens_ttl(self):
        ns = ContextNamespace(config=NamespaceConfig())
        append_records(ns, 4)
        view = ns.render()
        summarize_records_sync(
            ns,
            render_id=view.render_id,
            start_index=0,
            end_index=2,
            label="batch",
        )
        receipt = ns.expand("/batch", ttl_renders=3)
        assert receipt.ttl_renders == 3

        receipt2 = ns.expand("/batch", ttl_renders=1)
        assert receipt2.ttl_renders == 3


class TestSnapshotRestoreWithExpansions:
    """Test that expansion state survives snapshot/restore."""

    def test_expansion_survives_snapshot_restore(self):
        ns = ContextNamespace(config=NamespaceConfig())
        append_records(ns, 4)
        view = ns.render()
        summarize_records_sync(
            ns,
            render_id=view.render_id,
            start_index=0,
            end_index=2,
            label="batch",
        )
        ns.expand("/batch", ttl_renders=3)
        snap = ns.snapshot()
        restored = ContextNamespace.restore(snap)
        assert restored.is_expanded("/batch") is True

    def test_collapsed_state_survives_snapshot_restore(self):
        ns = ContextNamespace(config=NamespaceConfig())
        append_records(ns, 4)
        view = ns.render()
        summarize_records_sync(
            ns,
            render_id=view.render_id,
            start_index=0,
            end_index=2,
            label="batch",
        )
        ns.expand("/batch", ttl_renders=3)
        ns.collapse("/batch")
        snap = ns.snapshot()
        restored = ContextNamespace.restore(snap)
        assert restored.is_expanded("/batch") is False


class TestSnapshotRestoreRoundtrip:
    """Test snapshot/restore fidelity."""

    def test_restore_preserves_version_and_record_count(self):
        ns = ContextNamespace(config=NamespaceConfig())
        append_records(ns, 5)
        view = ns.render()
        summarize_records_sync(
            ns,
            render_id=view.render_id,
            start_index=0,
            end_index=3,
            label="batch",
        )
        snap = ns.snapshot()
        restored = ContextNamespace.restore(snap)
        assert restored.version == ns.version
        assert restored.stats().records == ns.stats().records
        assert restored.stats().nodes == ns.stats().nodes

    def test_restore_then_append_continues_version(self):
        ns = ContextNamespace(config=NamespaceConfig())
        append_records(ns, 3)
        snap = ns.snapshot()
        restored = ContextNamespace.restore(snap)
        v_before = restored.version
        restored.append(make_record("new"))
        assert restored.version == v_before + 1

    def test_restore_then_render_works(self):
        ns = ContextNamespace(config=NamespaceConfig())
        append_records(ns, 3)
        snap = ns.snapshot()
        restored = ContextNamespace.restore(snap)
        view = restored.render()
        assert view.entries == 3


class TestTextSliceSummarization:
    """Test text-slice summarization end-to-end."""

    def test_summarize_text_creates_node(self):
        ns = ContextNamespace(config=NamespaceConfig())
        ns.append(make_record("hello world this is a long text"))
        view = ns.render()
        receipt = summarize_text_sync(
            ns,
            render_id=view.render_id,
            record_index=0,
            start_char=0,
            end_char=11,
            label="text-slice",
        )
        assert receipt.node_id
        assert ns.stats().nodes == 1
        nodes = ns.list_nodes()
        assert nodes[0].source_kind == "TextSlice"
        assert nodes[0].path == "/text-slice"

    def test_summarize_text_preserves_remaining_content(self):
        ns = ContextNamespace(config=NamespaceConfig())
        ns.append(make_record("hello world remaining content"))
        view = ns.render()
        summarize_text_sync(
            ns,
            render_id=view.render_id,
            record_index=0,
            start_char=0,
            end_char=11,
            label="slice",
        )
        view2 = ns.render()
        assert "remaining content" in view2.json

    def test_summarize_text_stale_render_raises(self):
        from context_namespace import StaleRenderError

        ns = ContextNamespace(config=NamespaceConfig())
        ns.append(make_record("hello world"))
        view = ns.render()
        ns.append(make_record("new record"))
        with pytest.raises(StaleRenderError):
            summarize_text_sync(
                ns,
                render_id=view.render_id,
                record_index=0,
                start_char=0,
                end_char=5,
                label="slice",
            )


class TestTextSliceAndRecordRangeInteraction:
    """Test combining text-slice and record-range summarization."""

    def test_text_slice_then_record_range(self):
        ns = ContextNamespace(config=NamespaceConfig())
        ns.append(make_record("long text here for slicing"))
        append_records(ns, 3, prefix="extra")
        view = ns.render()

        summarize_text_sync(
            ns,
            render_id=view.render_id,
            record_index=0,
            start_char=0,
            end_char=14,
            label="text-node",
        )
        assert ns.stats().nodes == 1

        view2 = ns.render()
        summarize_records_sync(
            ns,
            render_id=view2.render_id,
            start_index=0,
            end_index=2,
            label="record-node",
        )
        assert ns.stats().nodes == 2

    def test_record_range_then_text_slice(self):
        ns = ContextNamespace(config=NamespaceConfig())
        append_records(ns, 5)
        view = ns.render()
        summarize_records_sync(
            ns,
            render_id=view.render_id,
            start_index=0,
            end_index=3,
            label="record-node",
        )
        assert ns.stats().nodes == 1

        view2 = ns.render()
        summarize_text_sync(
            ns,
            render_id=view2.render_id,
            record_index=1,
            start_char=0,
            end_char=5,
            label="text-node",
        )
        assert ns.stats().nodes == 2


class CapturingSummarizer:
    """Records each SummaryInput and returns a fixed, recognizable result."""

    def __init__(self) -> None:
        self.inputs: list[SummaryInput] = []

    async def summarize(self, inp: SummaryInput) -> SummaryResult:
        self.inputs.append(inp)
        return SummaryResult(summary="captured summary", gloss="captured gloss")


class TestSummarizerChildDescriptors:
    """Verify summarizer callbacks receive the selection's child descriptors."""

    @staticmethod
    def _summarize_cd(ns: ContextNamespace) -> tuple[str, CapturingSummarizer]:
        view = ns.render()
        summarizer = CapturingSummarizer()
        receipt = summarize_text_sync(
            ns,
            render_id=view.render_id,
            record_index=0,
            start_char=2,
            end_char=4,
            label="cd",
            summarizer=summarizer,
        )
        return receipt.node_id, summarizer

    def test_text_selection_spanning_a_reference_passes_child_descriptors(self):
        ns = ContextNamespace(config=NamespaceConfig())
        ns.append(make_record("abcdef"))
        cd_id, cd_summarizer = self._summarize_cd(ns)

        parent = CapturingSummarizer()
        view = ns.render()
        summarize_text_sync(
            ns,
            render_id=view.render_id,
            record_index=0,
            start_char=1,
            end_char=3,
            label="parent",
            summarizer=parent,
        )

        expected = ChildDescriptor(
            node_id=cd_id,
            label="cd",
            summary="captured summary",
            gloss="captured gloss",
        )
        (inp,) = parent.inputs
        assert inp.source == "be"
        assert inp.children == (expected,)
        assert cd_summarizer.inputs[0].children == ()
        assert {n.path for n in ns.list_nodes()} >= {"/parent", "/parent/cd"}

    def test_record_range_selection_containing_a_node_passes_child_descriptors(self):
        ns = ContextNamespace(config=NamespaceConfig())
        ns.append(make_record("alpha"))
        ns.append(make_record("beta"))
        view = ns.render()
        cd = summarize_records_sync(
            ns,
            render_id=view.render_id,
            start_index=0,
            end_index=1,
            label="batch",
        )

        parent = CapturingSummarizer()
        view2 = ns.render()
        summarize_records_sync(
            ns,
            render_id=view2.render_id,
            start_index=0,
            end_index=2,
            label="parent",
            summarizer=parent,
        )

        (inp,) = parent.inputs
        assert [c.node_id for c in inp.children] == [cd.node_id]
        assert inp.children[0].label == "batch"


class TestMaintenanceLifecycle:
    """Test the full maintenance workflow."""

    def test_maintain_no_action_with_few_records(self):
        ns = ContextNamespace(config=NamespaceConfig())
        append_records(ns, 3)
        result = maintain_sync(ns, estimated_next_tokens=100, per_record_tokens=50)
        assert result.action == "no_action"
        assert result.receipt is None

    def test_maintain_summarize_with_many_records(self):
        ns = ContextNamespace(config=NamespaceConfig())
        for i in range(12):
            ns.append(make_record(f"record {i} with some content here"))
        result = maintain_sync(
            ns, estimated_next_tokens=50_000, per_record_tokens=5_000
        )
        assert result.action == "summarized"
        assert result.receipt is not None
        assert result.receipt.node_id

    def test_maintain_summarizer_not_called_on_no_action(self):
        ns = ContextNamespace(config=NamespaceConfig())
        append_records(ns, 3)
        summarizer = CountingSummarizer()
        maintain_sync(
            ns,
            estimated_next_tokens=100,
            per_record_tokens=50,
            summarizer=summarizer,
        )
        assert summarizer.call_count == 0


class TestRenderDeterminism:
    """Test that renders are deterministic for the same state."""

    def test_consecutive_renders_match_entries_and_bytes(self):
        ns = ContextNamespace(config=NamespaceConfig())
        append_records(ns, 5)
        view1 = ns.render()
        view2 = ns.render()
        assert view1.entries == view2.entries
        assert view1.bytes == view2.bytes

    def test_render_after_restore_matches_entries(self):
        ns = ContextNamespace(config=NamespaceConfig())
        append_records(ns, 5)
        ns.render()
        snap = ns.snapshot()
        restored = ContextNamespace.restore(snap)
        v1 = restored.render()
        v2 = restored.render()
        assert v1.entries == v2.entries
        assert v1.bytes == v2.bytes
