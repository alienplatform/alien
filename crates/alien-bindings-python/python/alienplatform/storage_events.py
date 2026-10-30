"""Decode portable storage notifications without acknowledging queue deliveries."""

from __future__ import annotations

import json
import re
from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Literal, Protocol, cast

from .errors import AlienError

StorageEventType = Literal[
    "created", "deleted", "copied", "metadataUpdated", "restored", "tierChanged", "unknown"
]
_EVENT_TYPES = {
    "created", "deleted", "copied", "metadataUpdated", "restored", "tierChanged", "unknown"
}
_TIMESTAMP = re.compile(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})")


@dataclass(frozen=True)
class StorageEvent:
    """A validated notification; the object remains the authoritative record."""

    event_type: StorageEventType
    bucket_name: str
    object_key: str
    timestamp: datetime
    size: int | None = None
    etag: str | None = None
    content_type: str | None = None
    metadata: dict[str, str] = field(default_factory=dict)
    copy_source: str | None = None
    previous_tier: str | None = None
    current_tier: str | None = None
    region: str | None = None
    version_id: str | None = None


class StorageEventMessage(Protocol):
    """Payload surface shared by binding messages and Worker queue deliveries."""

    @property
    def payload(self) -> str | bytes: ...


def _string(value: dict[str, object], name: str) -> str:
    result = value.get(name)
    if not isinstance(result, str):
        raise ValueError(f"{name} must be a string")
    return result


def _optional_string(value: dict[str, object], name: str) -> str | None:
    if value.get(name) is None:
        return None
    return _string(value, name)


def storage_event_from_message(message: StorageEventMessage) -> StorageEvent:
    """Parse a binding or single/batched Worker message, leaving acknowledgment to the caller.

    Invalid JSON and invalid schema fields raise INVALID_STORAGE_EVENT_MESSAGE.
    Wire field names are camelCase; the returned Python attributes are snake_case.
    """
    try:
        payload: object = json.loads(message.payload)
        if not isinstance(payload, dict):
            raise ValueError("payload must be a JSON object")
        value = cast(dict[str, object], payload)
        event_type = _string(value, "eventType")
        if event_type not in _EVENT_TYPES:
            raise ValueError("eventType is not a recognized storage event type")
        timestamp = _string(value, "timestamp")
        if _TIMESTAMP.fullmatch(timestamp) is None:
            raise ValueError("timestamp must be RFC3339 with a timezone")
        parsed_time = datetime.fromisoformat(timestamp.replace("Z", "+00:00"))
        size = value.get("size")
        if size is not None and (type(size) is not int or not 0 <= size <= 2**64 - 1):
            raise ValueError("size must be an unsigned 64-bit integer")
        metadata = value.get("metadata", {})
        if not isinstance(metadata, dict) or not all(
            isinstance(key, str) and isinstance(item, str) for key, item in metadata.items()
        ):
            raise ValueError("metadata must map strings to strings")
        return StorageEvent(
            event_type=cast(StorageEventType, event_type),
            bucket_name=_string(value, "bucketName"),
            object_key=_string(value, "objectKey"),
            timestamp=parsed_time.astimezone(timezone.utc),
            size=cast(int | None, size),
            etag=_optional_string(value, "etag"),
            content_type=_optional_string(value, "contentType"),
            metadata=dict(metadata),
            copy_source=_optional_string(value, "copySource"),
            previous_tier=_optional_string(value, "previousTier"),
            current_tier=_optional_string(value, "currentTier"),
            region=_optional_string(value, "region"),
            version_id=_optional_string(value, "versionId"),
        )
    except (ValueError, TypeError, UnicodeError) as error:
        raise AlienError(
            "Queue payload is not a valid StorageEvent",
            code="INVALID_STORAGE_EVENT_MESSAGE",
            context={"reason": str(error)},
            retryable=False,
            internal=False,
            http_status_code=400,
        ) from error
