"""Configuration models for the context-namespace Python facade.

This module provides frozen, slotted dataclasses for all namespace
configuration types. These mirror the Rust ``NamespaceConfig`` and its
sub-configurations but use plain Python dataclasses so the wheel has no
third-party runtime dependency.

Important Classes:
    IndexMode: Enum controlling which index metadata is injected into rendered
        records.
    CarrierTemplate: Template for synthetic carrier records.
    PolicyConfig: Maintenance policy configuration enforced by Rust.
    RenderOptions: Options controlling rendered JSON output.
    DefensiveLimits: Defensive ceilings for all namespace collections and
        outputs.
    NamespaceConfig: Top-level configuration for a ContextNamespace.
"""

from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum


class IndexMode(StrEnum):
    """Controls which index metadata is injected into rendered records.

    Attributes:
        NONE: No index metadata injected.
        RECORD_ID: Inject the record ID only.
        VISIBLE_INDEX: Inject the visible index only.
        BOTH: Inject both the visible index and record ID.
    """

    NONE = "None"
    RECORD_ID = "RecordId"
    VISIBLE_INDEX = "VisibleIndex"
    BOTH = "Both"


@dataclass(frozen=True, slots=True)
class CarrierTemplate:
    """Declarative template for synthetic carrier records.

    When a record range is summarized, the range is replaced by one
    synthetic carrier record. This template controls how that record is
    constructed.

    Attributes:
        role: The role field value for carrier records.
        format: The format string for the carrier's render marker.
    """

    role: str = "system"
    format: str = "[summary: {gloss}]"


@dataclass(frozen=True, slots=True)
class PolicyConfig:
    """Configuration for the maintenance policy.

    Attributes:
        effective_budget_tokens: Effective token budget for rendered history.
        trigger_tokens: Trigger threshold; summarization is considered when
            the next render exceeds this many tokens.
        target_tokens: Target token count for the summarized range.
        preserve_prefix_records: Number of prefix records always preserved
            from summarization.
        preserve_recent_records: Number of most-recent records always
            preserved from summarization.
        max_summary_chars: Maximum character length for summaries.
        max_gloss_chars: Maximum character length for glosses.
        default_expansion_ttl: Default TTL (in renders) for expansions.
        max_expansion_ttl: Maximum TTL (in renders) for any expansion.
    """

    effective_budget_tokens: int = 32_000
    trigger_tokens: int = 24_000
    target_tokens: int = 16_000
    preserve_prefix_records: int = 1
    preserve_recent_records: int = 8
    max_summary_chars: int = 1_200
    max_gloss_chars: int = 160
    default_expansion_ttl: int = 2
    max_expansion_ttl: int = 4


@dataclass(frozen=True, slots=True)
class RenderOptions:
    """Options controlling rendered JSON output.

    Attributes:
        index_mode: Controls which index metadata is injected.
        envelope: Whether to wrap rendered records in a JSON envelope.
        pretty: Whether to produce pretty-printed JSON.
        metadata_key: The reserved metadata key for ``__ctx`` metadata.
        include_child_glosses: Whether to include child gloss lists in
            collapsed summary markers.
    """

    index_mode: IndexMode = IndexMode.BOTH
    envelope: bool = True
    pretty: bool = False
    metadata_key: str = "__ctx"
    include_child_glosses: bool = True


@dataclass(frozen=True, slots=True)
class DefensiveLimits:
    """Defensive ceilings for all namespace collections and outputs.

    These are defensive limits, not benchmark hyperparameters. They are
    enforced before allocation where possible and rechecked on
    commit/render.

    Attributes:
        max_records: Maximum number of canonical records.
        max_nodes: Maximum number of summary nodes in the tree.
        max_source_bytes: Maximum total bytes of source material.
        max_render_bytes: Maximum byte length of a single rendered output.
        max_record_bytes: Maximum byte length of a single record's render
            field.
        max_summary_chars: Maximum character length of a summary text.
        max_gloss_chars: Maximum character length of a gloss text.
        max_tree_depth: Maximum depth of the summary tree.
        max_children_per_node: Maximum number of immediate children per node.
        max_active_expansions: Maximum number of simultaneously active
            expansions.
        max_pending_requests: Maximum number of pending summary requests.
        max_aliases_per_node: Maximum number of retained path aliases per
            node.
        retained_render_resolutions: Number of recent render resolutions
            retained for stale-render detection.
        max_label_length: Maximum character length of a namespace label.
        max_event_queue: Maximum number of events in the bounded event queue.
    """

    max_records: int = 10_000
    max_nodes: int = 1_000
    max_source_bytes: int = 4_194_304
    max_render_bytes: int = 4_194_304
    max_record_bytes: int = 524_288
    max_summary_chars: int = 1_200
    max_gloss_chars: int = 160
    max_tree_depth: int = 64
    max_children_per_node: int = 64
    max_active_expansions: int = 16
    max_pending_requests: int = 4
    max_aliases_per_node: int = 16
    retained_render_resolutions: int = 4
    max_label_length: int = 64
    max_event_queue: int = 256

    @classmethod
    def portfolio_defaults(cls) -> DefensiveLimits:
        """Returns the portfolio default limits.

        Returns:
            A DefensiveLimits instance with defaults suitable for agent
            benchmarking.
        """
        return cls()


@dataclass(frozen=True, slots=True)
class NamespaceConfig:
    """Top-level configuration for a ContextNamespace.

    Attributes:
        render_field: The string field name that the model sees.
        carrier: Template for synthetic carrier records.
        render: Options controlling rendered JSON output.
        policy: Maintenance policy configuration enforced by Rust.
        limits: Defensive ceilings for all namespace collections and
            outputs.
    """

    render_field: str = "content"
    carrier: CarrierTemplate = CarrierTemplate()
    render: RenderOptions = RenderOptions()
    policy: PolicyConfig = PolicyConfig()
    limits: DefensiveLimits = DefensiveLimits()
