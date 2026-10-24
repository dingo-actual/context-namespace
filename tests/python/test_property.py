"""Property-based tests for the Python facade using Hypothesis.

These tests use Hypothesis strategies to generate inputs and verify
general properties that should hold across the input domain. The
properties cover invariants, postconditions, round-trip relations,
and stateful model-based testing.

Property forms used:
    - Invariant: validate() passes after any operation sequence
    - Postcondition: append increments version by exactly 1
    - Round-trip: snapshot then restore preserves state
    - Metamorphic: consecutive renders produce identical entries/bytes
    - Stateful model: operation sequences preserve invariants
"""

from __future__ import annotations

from conftest import (
    make_record,
    summarize_records_sync,
)
from context_namespace import (
    ContextNamespace,
    ContextNamespaceError,
    NamespaceConfig,
)
from hypothesis import HealthCheck, given, settings
from hypothesis import strategies as st
from hypothesis.stateful import (
    RuleBasedStateMachine,
    invariant,
    rule,
    run_state_machine_as_test,
)

# ---------------------------------------------------------------------------
# Strategies
# ---------------------------------------------------------------------------


@st.composite
def valid_record(draw):
    """Generates a valid record dict with role and content strings.

    Args:
        draw: The Hypothesis draw function.

    Returns:
        A dict with 'role' and 'content' keys.
    """
    role = draw(st.sampled_from(["user", "assistant", "system"]))
    content = draw(st.text(min_size=1, max_size=100))
    return {"role": role, "content": content}


@st.composite
def valid_label(draw):
    """Generates a valid namespace label string.

    Labels must be non-empty and reasonably short.

    Args:
        draw: The Hypothesis draw function.

    Returns:
        A non-empty string suitable for use as a path label.
    """
    return draw(
        st.text(
            alphabet=st.characters(
                whitelist_categories=("Ll", "Lu", "Nd"),
                whitelist_characters="-_",
            ),
            min_size=1,
            max_size=20,
        )
    )


@st.composite
def records_strategy(draw, max_count=10):
    """Generates a list of valid records.

    Args:
        draw: The Hypothesis draw function.
        max_count: Maximum number of records to generate.

    Returns:
        A list of record dicts.
    """
    count = draw(st.integers(min_value=0, max_value=max_count))
    return [draw(valid_record()) for _ in range(count)]


# ---------------------------------------------------------------------------
# Invariant properties
# ---------------------------------------------------------------------------


class TestInvariantProperties:
    """Properties asserting invariants hold across generated inputs."""

    @given(records=records_strategy(max_count=10))
    @settings(max_examples=30, deadline=None)
    def test_validate_passes_after_appends(self, records):
        """validate() passes after any sequence of appends."""
        ns = ContextNamespace(config=NamespaceConfig())
        for r in records:
            ns.append(r)
        ns.validate()

    @given(records=records_strategy(max_count=10))
    @settings(max_examples=30, deadline=None)
    def test_version_equals_record_count(self, records):
        """version equals the number of appended records."""
        ns = ContextNamespace(config=NamespaceConfig())
        for r in records:
            ns.append(r)
        assert ns.version == len(records)

    @given(records=records_strategy(max_count=10))
    @settings(max_examples=30, deadline=None)
    def test_stats_records_equals_len(self, records):
        """stats().records equals the number of appended records."""
        ns = ContextNamespace(config=NamespaceConfig())
        for r in records:
            ns.append(r)
        assert ns.stats().records == len(records)


# ---------------------------------------------------------------------------
# Postcondition properties
# ---------------------------------------------------------------------------


class TestPostconditionProperties:
    """Properties relating inputs to outputs and post-state."""

    @given(
        content=st.text(min_size=1, max_size=200),
        role=st.sampled_from(["user", "assistant", "system"]),
    )
    @settings(max_examples=30, deadline=None)
    def test_append_returns_incrementing_visible_index(self, content, role):
        """Each append returns a visible_index incremented by 1."""
        ns = ContextNamespace(config=NamespaceConfig())
        receipt = ns.append({"role": role, "content": content})
        assert receipt.visible_index == 0
        receipt2 = ns.append({"role": role, "content": content + "x"})
        assert receipt2.visible_index == 1

    @given(records=records_strategy(max_count=8))
    @settings(max_examples=30, deadline=None)
    def test_render_entries_equals_record_count(self, records):
        """render().entries equals the number of records."""
        ns = ContextNamespace(config=NamespaceConfig())
        for r in records:
            ns.append(r)
        view = ns.render()
        assert view.entries == len(records)

    @given(records=records_strategy(max_count=8))
    @settings(max_examples=30, deadline=None)
    def test_render_bytes_positive_with_records(self, records):
        """render().bytes is positive when records exist."""
        if not records:
            return
        ns = ContextNamespace(config=NamespaceConfig())
        for r in records:
            ns.append(r)
        view = ns.render()
        assert view.bytes > 0


# ---------------------------------------------------------------------------
# Round-trip properties
# ---------------------------------------------------------------------------


class TestRoundTripProperties:
    """Properties verifying serialize/deserialize round-trips."""

    @given(records=records_strategy(max_count=8))
    @settings(max_examples=30, deadline=None)
    def test_snapshot_restore_preserves_version(self, records):
        """snapshot then restore preserves the namespace version."""
        ns = ContextNamespace(config=NamespaceConfig())
        for r in records:
            ns.append(r)
        snap = ns.snapshot()
        restored = ContextNamespace.restore(snap)
        assert restored.version == ns.version

    @given(records=records_strategy(max_count=8))
    @settings(max_examples=30, deadline=None)
    def test_snapshot_restore_preserves_record_count(self, records):
        """snapshot then restore preserves the record count."""
        ns = ContextNamespace(config=NamespaceConfig())
        for r in records:
            ns.append(r)
        snap = ns.snapshot()
        restored = ContextNamespace.restore(snap)
        assert restored.stats().records == ns.stats().records

    @given(records=records_strategy(max_count=8))
    @settings(max_examples=30, deadline=None)
    def test_snapshot_restore_preserves_node_count(self, records):
        """snapshot then restore preserves the node count."""
        ns = ContextNamespace(config=NamespaceConfig())
        for r in records:
            ns.append(r)
        if len(records) >= 4:
            view = ns.render()
            summarize_records_sync(
                ns,
                render_id=view.render_id,
                start_index=0,
                end_index=min(3, len(records) - 1),
                label="batch",
            )
        snap = ns.snapshot()
        restored = ContextNamespace.restore(snap)
        assert restored.stats().nodes == ns.stats().nodes

    @given(records=records_strategy(max_count=4))
    @settings(max_examples=20, deadline=None)
    def test_double_snapshot_restore_is_idempotent(self, records):
        """Restoring from a restored namespace yields identical state."""
        ns = ContextNamespace(config=NamespaceConfig())
        for r in records:
            ns.append(r)
        snap1 = ns.snapshot()
        r1 = ContextNamespace.restore(snap1)
        snap2 = r1.snapshot()
        r2 = ContextNamespace.restore(snap2)
        assert r2.version == r1.version
        assert r2.stats().records == r1.stats().records


# ---------------------------------------------------------------------------
# Metamorphic properties
# ---------------------------------------------------------------------------


class TestMetamorphicProperties:
    """Properties comparing multiple executions under transformations."""

    @given(records=records_strategy(max_count=8))
    @settings(max_examples=30, deadline=None)
    def test_consecutive_renders_match(self, records):
        """Consecutive renders produce identical entries and bytes."""
        ns = ContextNamespace(config=NamespaceConfig())
        for r in records:
            ns.append(r)
        v1 = ns.render()
        v2 = ns.render()
        assert v1.entries == v2.entries
        assert v1.bytes == v2.bytes

    @given(records=records_strategy(max_count=8))
    @settings(max_examples=30, deadline=None)
    def test_render_after_restore_matches_original(self, records):
        """Render after restore matches the original render."""
        ns = ContextNamespace(config=NamespaceConfig())
        for r in records:
            ns.append(r)
        v1 = ns.render()
        snap = ns.snapshot()
        restored = ContextNamespace.restore(snap)
        v2 = restored.render()
        assert v1.entries == v2.entries
        assert v1.bytes == v2.bytes


# ---------------------------------------------------------------------------
# Error properties
# ---------------------------------------------------------------------------


class TestErrorProperties:
    """Properties verifying error behavior across inputs."""

    @given(data=st.binary(min_size=0, max_size=64))
    @settings(
        max_examples=20,
        deadline=None,
        suppress_health_check=[HealthCheck.too_slow],
    )
    def test_malformed_restore_does_not_panic(self, data):
        """Restoring arbitrary bytes returns an error, never panics."""
        try:
            ContextNamespace.restore(data)
        except ContextNamespaceError:
            pass

    @given(label=valid_label())
    @settings(max_examples=20, deadline=None)
    def test_duplicate_label_always_raises_path_collision(self, label):
        """Summarizing with a duplicate label always raises an error."""
        from context_namespace import PathCollisionError

        ns = ContextNamespace(config=NamespaceConfig())
        append_count = 6
        for i in range(append_count):
            ns.append(make_record(f"record {i}"))
        view = ns.render()
        summarize_records_sync(
            ns,
            render_id=view.render_id,
            start_index=0,
            end_index=3,
            label=label,
        )
        view2 = ns.render()
        try:
            summarize_records_sync(
                ns,
                render_id=view2.render_id,
                start_index=0,
                end_index=1,
                label=label,
            )
            raise AssertionError("expected PathCollisionError")
        except PathCollisionError:
            pass


# ---------------------------------------------------------------------------
# Stateful model-based testing
# ---------------------------------------------------------------------------


class NamespaceStateMachine(RuleBasedStateMachine):
    """Stateful model comparing ContextNamespace against a simple model.

    The model tracks record count and version. Rules append records,
    render, and snapshot/restore. Invariants verify the model and the
    real namespace agree after each step.
    """

    def __init__(self):
        super().__init__()
        self.ns = ContextNamespace(config=NamespaceConfig())
        self.model_version: int = 0
        self.model_records: int = 0

    @rule(content=st.text(min_size=1, max_size=50))
    def append(self, content):
        self.ns.append({"role": "user", "content": content})
        self.model_version += 1
        self.model_records += 1

    @rule()
    def render(self):
        view = self.ns.render()
        assert view.entries == self.model_records
        assert view.namespace_version == self.model_version

    @rule()
    def snapshot_restore(self):
        snap = self.ns.snapshot()
        restored = ContextNamespace.restore(snap)
        assert restored.version == self.model_version
        assert restored.stats().records == self.model_records

    @invariant()
    def version_matches_model(self):
        assert self.ns.version == self.model_version

    @invariant()
    def records_match_model(self):
        assert self.ns.stats().records == self.model_records

    @invariant()
    def validate_passes(self):
        self.ns.validate()


class TestStatefulModel:
    """Run the stateful model-based test."""

    def test_stateful_namespace_model(self):
        run_state_machine_as_test(
            NamespaceStateMachine,
            settings=settings(
                max_examples=30,
                stateful_step_count=20,
                deadline=None,
            ),
        )
