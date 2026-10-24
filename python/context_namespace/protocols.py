"""Protocols for the context-namespace Python facade.

These runtime-checkable protocols define the contracts that user-supplied
summarizer and embedder implementations must satisfy.

Important Classes:
    Summarizer: Protocol for an async summarizer.
    Embedder: Protocol for an async embedder.
"""

from __future__ import annotations

from collections.abc import Sequence
from typing import Protocol, runtime_checkable

from .models import EmbeddingModelSpec, SummaryInput, SummaryResult


@runtime_checkable
class Summarizer(Protocol):
    """Protocol for an async summarizer.

    Implementations call a model SDK to produce a summary and gloss
    from a SummaryInput. The library calls ``summarize`` between
    prepare and commit, so no mutable native state is held during
    inference.
    """

    async def summarize(self, request: SummaryInput) -> SummaryResult: ...


@runtime_checkable
class Embedder(Protocol):
    """Protocol for an async embedder.

    Implementations call an embedding model to produce vectors from
    text. The ``model_spec`` property identifies the model and
    dimensions so the library can validate batch compatibility.

    Attributes:
        model_spec: The embedding model specification.
    """

    @property
    def model_spec(self) -> EmbeddingModelSpec: ...

    async def embed(self, texts: Sequence[str]) -> list[list[float]]: ...
