"""Edge-case and boundary-value tests for the Python facade.

Covers empty namespace operations, defensive limit boundaries,
index-range validation, from_records constructor, preview_render
fields, drain_events content verification, and stats full-field
verification.
"""

from __future__ import annotations

import json

import pytest
from conftest import (
    append_records,
    make_record,
    make_tight_config,
    summarize_records_sync,
)
from context_namespace import (
    ContextNamespace,
    ContextNamespaceError,
    InvalidSelectionError,
    InvariantViolationError,
    NamespaceConfig,
    UnknownPathError,
)


class TestEmptyNamespace:
    """Verify operations on a namespace with zero records."""

    def test_render_on_empty_namespace_returns_zero_entries(self):
        ns = ContextNamespace(config=NamespaceConfig())
        view = ns.render()
        assert view.entries == 0
        assert view.bytes > 0
        assert view.namespace_version == 0
        assert view.render_id

    def test_stats_on_empty_namespace(self):
        ns = ContextNamespace(config=NamespaceConfig())
        s = ns.stats()
        assert s.records == 0
        assert s.nodes == 0
        assert s.active_expansions == 0
        assert s.pending_requests == 0
        assert s.alias_count == 0
        assert s.max_depth == 0
        assert s.dropped_events == 0

    def test_list_nodes_on_empty_namespace(self):
        ns = ContextNamespace(config=NamespaceConfig())
        assert ns.list_nodes() == []

    def test_validate_on_empty_namespace(self):
        ns = ContextNamespace(config=NamespaceConfig())
        ns.validate()

    def test_drain_events_on_empty_namespace(self):
        ns = ContextNamespace(config=NamespaceConfig())
        assert ns.drain_events() == []

    def test_preview_render_on_empty_namespace(self):
        ns = ContextNamespace(config=NamespaceConfig())
        preview = ns.preview_render()
        assert preview.entries == 0
        assert preview.bytes > 0
        assert preview.namespace_version == 0
        assert preview.maintenance_units == ()

    def test_version_on_empty_namespace_is_zero(self):
        ns = ContextNamespace(config=NamespaceConfig())
        assert ns.version == 0

    def test_snapshot_on_empty_namespace(self):
        ns = ContextNamespace(config=NamespaceConfig())
        snap = ns.snapshot()
        assert len(snap) > 0
        restored = ContextNamespace.restore(snap)
        assert restored.stats().records == 0
        assert restored.version == 0

    def test_expand_on_empty_namespace_raises_unknown_path(self):
        ns = ContextNamespace(config=NamespaceConfig())
        with pytest.raises(UnknownPathError):
            ns.expand("/nonexistent", ttl_renders=1)

    def test_collapse_on_empty_namespace_raises_unknown_path(self):
        ns = ContextNamespace(config=NamespaceConfig())
        with pytest.raises(UnknownPathError):
            ns.collapse("/nonexistent")


class TestDefensiveLimitBoundaries:
    """Exercise defensive limits at and over their boundaries."""

    def test_max_records_at_limit_does_not_raise(self):
        config = make_tight_config(max_records=3)
        ns = ContextNamespace(config=config)
        for i in range(3):
            ns.append(make_record(f"record {i}"))
        assert ns.stats().records == 3

    def test_max_records_over_limit_raises(self):
        config = make_tight_config(max_records=3)
        ns = ContextNamespace(config=config)
        for i in range(3):
            ns.append(make_record(f"record {i}"))
        with pytest.raises(ContextNamespaceError):
            ns.append(make_record("overflow"))

    def test_max_records_limit_of_one(self):
        config = make_tight_config(max_records=1)
        ns = ContextNamespace(config=config)
        ns.append(make_record("only one"))
        with pytest.raises(ContextNamespaceError):
            ns.append(make_record("too many"))


class TestIndexRangeValidation:
    """Test summarize_records with invalid index ranges."""

    def test_inverted_range_raises(self):
        ns = ContextNamespace(config=NamespaceConfig())
        append_records(ns, 4)
        view = ns.render()
        with pytest.raises(InvalidSelectionError):
            summarize_records_sync(
                ns,
                render_id=view.render_id,
                start_index=2,
                end_index=1,
                label="bad",
            )

    def test_end_index_beyond_record_count_raises(self):
        ns = ContextNamespace(config=NamespaceConfig())
        append_records(ns, 3)
        view = ns.render()
        with pytest.raises(InvalidSelectionError):
            summarize_records_sync(
                ns,
                render_id=view.render_id,
                start_index=0,
                end_index=10,
                label="bad",
            )

    def test_start_index_equal_to_end_index_raises(self):
        ns = ContextNamespace(config=NamespaceConfig())
        append_records(ns, 4)
        view = ns.render()
        with pytest.raises(InvalidSelectionError):
            summarize_records_sync(
                ns,
                render_id=view.render_id,
                start_index=1,
                end_index=1,
                label="bad",
            )


class TestFromRecordsConstructor:
    """Test ContextNamespace with initial records."""

    def test_from_records_with_data(self):
        records = [
            {"role": "user", "content": "hello"},
            {"role": "assistant", "content": "world"},
        ]
        ns = ContextNamespace(config=NamespaceConfig(), records=records)
        assert ns.stats().records == 2
        assert ns.version == 2
        view = ns.render()
        assert view.entries == 2

    def test_from_records_empty_list(self):
        ns = ContextNamespace(config=NamespaceConfig(), records=[])
        assert ns.stats().records == 0
        assert ns.version == 0

    def test_from_records_then_append(self):
        ns = ContextNamespace(
            config=NamespaceConfig(),
            records=[{"role": "user", "content": "initial"}],
        )
        ns.append(make_record("appended"))
        assert ns.stats().records == 2
        assert ns.version == 2


class TestPreviewRenderFields:
    """Verify preview_render returns all expected fields."""

    def test_preview_render_fields(self):
        ns = ContextNamespace(config=NamespaceConfig())
        append_records(ns, 3)
        preview = ns.preview_render()
        assert preview.namespace_version == ns.version
        assert preview.json
        assert preview.bytes > 0
        assert preview.entries == 3
        assert len(preview.maintenance_units) == 3
        unit = preview.maintenance_units[0]
        assert unit.record_id
        assert unit.render_digest
        assert unit.json_fragment

    def test_preview_render_does_not_increment_version(self):
        ns = ContextNamespace(config=NamespaceConfig())
        ns.append(make_record("a"))
        v_before = ns.version
        ns.preview_render()
        assert ns.version == v_before

    def test_preview_render_does_not_emit_events(self):
        ns = ContextNamespace(config=NamespaceConfig())
        ns.append(make_record("a"))
        ns.drain_events()
        ns.preview_render()
        assert ns.drain_events() == []


class TestDrainEventsContent:
    """Verify event strings contain expected operation names."""

    def test_append_event_content(self):
        ns = ContextNamespace(config=NamespaceConfig())
        ns.append(make_record("hello"))
        events = ns.drain_events()
        assert len(events) == 1
        assert "Append" in events[0]

    def test_render_event_content(self):
        ns = ContextNamespace(config=NamespaceConfig())
        ns.append(make_record("hello"))
        ns.drain_events()
        ns.render()
        events = ns.drain_events()
        assert len(events) == 1
        assert "Render" in events[0]

    def test_summary_committed_event_content(self):
        ns = ContextNamespace(config=NamespaceConfig())
        append_records(ns, 4)
        view = ns.render()
        ns.drain_events()
        summarize_records_sync(
            ns,
            render_id=view.render_id,
            start_index=0,
            end_index=2,
            label="batch",
        )
        events = ns.drain_events()
        assert any("SummaryCommitted" in e for e in events)

    def test_event_drain_order_is_fifo(self):
        ns = ContextNamespace(config=NamespaceConfig())
        ns.append(make_record("a"))
        ns.append(make_record("b"))
        ns.render()
        events = ns.drain_events()
        assert len(events) == 3
        assert "Append" in events[0]
        assert "Append" in events[1]
        assert "Render" in events[2]


class TestStatsFullFields:
    """Verify all StructuralStats fields after relevant operations."""

    def test_stats_after_expand(self):
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
        assert ns.stats().nodes == 1
        ns.expand("/batch", ttl_renders=2)
        s = ns.stats()
        assert s.active_expansions == 1

    def test_stats_after_collapse(self):
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
        ns.expand("/batch", ttl_renders=2)
        ns.collapse("/batch")
        assert ns.stats().active_expansions == 0

    def test_stats_max_depth_after_nesting(self):
        ns = ContextNamespace(config=NamespaceConfig())
        append_records(ns, 8)
        view = ns.render()
        summarize_records_sync(
            ns,
            render_id=view.render_id,
            start_index=0,
            end_index=4,
            label="level-1",
        )
        view2 = ns.render()
        summarize_records_sync(
            ns,
            render_id=view2.render_id,
            start_index=0,
            end_index=2,
            label="level-2",
        )
        s = ns.stats()
        assert s.max_depth >= 2


class TestRestoreWithConfig:
    """Test restore with the optional config parameter."""

    def test_restore_with_config_does_not_raise(self):
        ns = ContextNamespace(config=NamespaceConfig())
        ns.append(make_record("hello"))
        snap = ns.snapshot()
        restored = ContextNamespace.restore(snap, config=NamespaceConfig())
        assert restored.stats().records == 1


class TestRestoreContentHash:
    """Test that restore verifies the snapshot content hash."""

    def test_untouched_snapshot_restores(self):
        ns = ContextNamespace(config=NamespaceConfig())
        ns.append(make_record("before"))
        restored = ContextNamespace.restore(ns.snapshot())
        assert "before" in restored.render().json

    def test_modified_record_content_is_rejected(self):
        ns = ContextNamespace(config=NamespaceConfig())
        ns.append(make_record("before"))
        envelope = json.loads(ns.snapshot())
        envelope["records"][0]["render"][0]["Text"] = "after"

        with pytest.raises(InvariantViolationError, match="content hash mismatch"):
            ContextNamespace.restore(json.dumps(envelope).encode())


class TestEventQueueOverflow:
    """Test bounded event queue overflow behavior."""

    def test_dropped_events_increments_on_overflow(self):
        config = make_tight_config(max_records=10, max_event_queue=2)
        ns = ContextNamespace(config=config)
        for i in range(5):
            ns.append(make_record(f"record {i}"))
        s = ns.stats()
        assert s.dropped_events > 0
