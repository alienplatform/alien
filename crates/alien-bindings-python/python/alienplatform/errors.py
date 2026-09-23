"""Structured exceptions raised by Alien resource bindings."""

from __future__ import annotations

import json
from collections.abc import Awaitable, Callable
from functools import wraps
from typing import Any, ParamSpec, TypeVar

P = ParamSpec("P")
T = TypeVar("T")


class AlienError(RuntimeError):
    """A provider-neutral Alien error with stable machine-readable metadata."""

    def __init__(
        self,
        message: str,
        *,
        code: str = "BINDINGS_ERROR",
        context: dict[str, Any] | None = None,
        retryable: bool = False,
        internal: bool = True,
        http_status_code: int | None = None,
        hint: str | None = None,
    ) -> None:
        super().__init__(message)
        self.code = code
        self.context = context
        self.retryable = retryable
        self.internal = internal
        self.http_status_code = http_status_code
        self.hint = hint

    @classmethod
    def from_native(cls, error: RuntimeError) -> AlienError:
        try:
            value = json.loads(str(error))
        except (json.JSONDecodeError, TypeError):
            return cls(str(error))
        return cls(
            value.get("message", str(error)),
            code=value.get("code", "BINDINGS_ERROR"),
            context=value.get("context"),
            retryable=value.get("retryable", False),
            internal=value.get("internal", True),
            http_status_code=value.get("httpStatusCode"),
            hint=value.get("hint"),
        )


def translate_errors(function: Callable[P, Awaitable[T]]) -> Callable[P, Awaitable[T]]:
    """Translate the native JSON error envelope into :class:`AlienError`."""

    @wraps(function)
    async def wrapped(*args: P.args, **kwargs: P.kwargs) -> T:
        try:
            return await function(*args, **kwargs)
        except RuntimeError as error:
            raise AlienError.from_native(error) from None

    return wrapped
