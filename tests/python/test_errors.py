"""Error mapping tests for the Python facade.

Verifies that native Rust exceptions are correctly mapped to the
Python exception hierarchy defined in errors.py. Each test triggers
a specific error condition and asserts the correct Python exception
type is raised.
"""

from __future__ import annotations

import pytest
from conftest import (
    FailingSummarizer,
    append_records,
    make_record,
    summarize_records_sync,
)
from context_namespace import (
    ContextNamespace,
    ContextNamespaceError,
    InvalidRecordError,
    InvalidSelectionError,
    InvariantViolationError,
    NamespaceConfig,
    PathCollisionError,
    ReservedKeyCollisionError,
    SnapshotVersionError,
    StaleRenderError,
    UnknownPathError,
)


class TestInvalidRecordError:
    """Tests for invalid record errors."""

    def test_missing_content_field_raises_invalid_record(self):
        ns = ContextNamespace(config=NamespaceConfig())
        with pytest.raises(InvalidRecordError):
            ns.append({"role": "user"})

    def test_non_string_content_raises_invalid_record(self):
        ns = ContextNamespace(config=NamespaceConfig())
        with pytest.raises(InvalidRecordError):
            ns.append({"role": "user", "content": 123})

    def test_reserved_key_collision_raises_error(self):
        ns = ContextNamespace(config=NamespaceConfig())
        with pytest.raises(ContextNamespaceError):
            ns.append({"role": "user", "content": "hello", "__ctx": "bad"})


class TestStaleRenderError:
    """Tests for stale render ID errors."""

    def test_stale_render_id_raises_stale_render_error(self):
        ns = ContextNamespace(config=NamespaceConfig())
        ns.append(make_record("a"))
        view = ns.render()
        ns.append(make_record("b"))
        with pytest.raises(StaleRenderError):
            summarize_records_sync(
                ns,
                render_id=view.render_id,
                start_index=0,
                end_index=1,
                label="bad",
            )

    def test_stale_render_error_is_subclass_of_invalid_selection(self):
        assert issubclass(StaleRenderError, InvalidSelectionError)


class TestUnknownPathError:
    """Tests for unknown path errors."""

    def test_expand_nonexistent_path_raises_unknown_path(self):
        ns = ContextNamespace(config=NamespaceConfig())
        with pytest.raises(UnknownPathError):
            ns.expand("/nope", ttl_renders=1)

    def test_collapse_nonexistent_path_raises_unknown_path(self):
        ns = ContextNamespace(config=NamespaceConfig())
        with pytest.raises(UnknownPathError):
            ns.collapse("/nope")

    def test_is_expanded_nonexistent_path_raises_unknown_path(self):
        ns = ContextNamespace(config=NamespaceConfig())
        with pytest.raises(UnknownPathError):
            ns.is_expanded("/nope")


class TestPathCollisionError:
    """Tests for path collision errors."""

    def test_duplicate_label_raises_path_collision(self):
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
        view2 = ns.render()
        with pytest.raises(PathCollisionError):
            summarize_records_sync(
                ns,
                render_id=view2.render_id,
                start_index=0,
                end_index=1,
                label="batch",
            )


class TestSnapshotVersionError:
    """Tests for snapshot version and restore errors."""

    def test_malformed_bytes_raises_error(self):
        with pytest.raises(ContextNamespaceError):
            ContextNamespace.restore(b"not valid json")

    def test_empty_bytes_raises_error(self):
        with pytest.raises(ContextNamespaceError):
            ContextNamespace.restore(b"")


class TestErrorHierarchy:
    """Verify the exception hierarchy structure."""

    def test_all_errors_inherit_from_context_namespace_error(self):
        for exc_type in [
            InvalidRecordError,
            InvalidSelectionError,
            StaleRenderError,
            UnknownPathError,
            PathCollisionError,
            SnapshotVersionError,
            InvariantViolationError,
        ]:
            assert issubclass(exc_type, ContextNamespaceError)

    def test_reserved_key_collision_is_subclass_of_invalid_record(self):
        assert issubclass(ReservedKeyCollisionError, InvalidRecordError)


class TestSummarizerErrorCancels:
    """Verify summarizer failures cancel pending state."""

    def test_runtime_error_propagates_and_cancels(self):
        ns = ContextNamespace(config=NamespaceConfig())
        append_records(ns, 4)
        view = ns.render()
        with pytest.raises(RuntimeError, match="inference failed"):
            summarize_records_sync(
                ns,
                render_id=view.render_id,
                start_index=0,
                end_index=2,
                label="batch",
                summarizer=FailingSummarizer(RuntimeError("inference failed")),
            )
        assert ns.stats().pending_requests == 0
        assert ns.stats().nodes == 0

    def test_value_error_propagates_and_cancels(self):
        ns = ContextNamespace(config=NamespaceConfig())
        append_records(ns, 4)
        view = ns.render()
        with pytest.raises(ValueError, match="bad model"):
            summarize_records_sync(
                ns,
                render_id=view.render_id,
                start_index=0,
                end_index=2,
                label="batch",
                summarizer=FailingSummarizer(ValueError("bad model")),
            )
        assert ns.stats().pending_requests == 0
