"""The same canonical payload is accepted from binding and Worker deliveries."""

import json
from datetime import datetime, timezone

import pytest
from alienplatform import AlienError, QueueEvent, QueueMessage, storage_event_from_message


def payload():
    return {
        "eventType": "created",
        "bucketName": "uploads",
        "objectKey": 'quote"é*.json',
        "timestamp": "2026-10-30T11:00:00+02:00",
        "size": 7,
        "etag": "version-1",
        "contentType": "application/json",
        "metadata": {"kind": "example"},
        "copySource": "source/key",
        "previousTier": "archive",
        "currentTier": "standard",
        "region": "region",
        "versionId": "revision-1",
    }


@pytest.mark.parametrize("worker", [False, True])
def test_storage_notification_preserves_fields_and_receipt(worker):
    value = json.dumps(payload())
    message = (
        QueueEvent("id", value.encode(), "jobs", 2)
        if worker
        else QueueMessage("json", value, "receipt", 2)
    )
    event = storage_event_from_message(message)
    assert event.event_type == "created"
    assert event.bucket_name == "uploads"
    assert event.object_key == 'quote"é*.json'
    assert event.timestamp == datetime(2026, 10, 30, 9, tzinfo=timezone.utc)
    assert event.size == 7
    assert event.etag == "version-1"
    assert event.content_type == "application/json"
    assert event.metadata == {"kind": "example"}
    assert event.copy_source == "source/key"
    assert event.previous_tier == "archive"
    assert event.current_tier == "standard"
    assert event.region == "region"
    assert event.version_id == "revision-1"
    assert message.payload == (value.encode() if worker else value)
    if isinstance(message, QueueMessage):
        assert message.receipt_handle == "receipt"
        assert message.attempt == 2


@pytest.mark.parametrize(
    ("timestamp", "expected"),
    [
        # Azure Event Grid sends 7 fractional digits, local mtimes 9; Python 3.10's
        # fromisoformat alone accepts only 3 or 6.
        ("2026-10-30T09:00:00.9584103Z", datetime(2026, 10, 30, 9, 0, 0, 958410, tzinfo=timezone.utc)),
        ("2026-10-30T09:00:00.958410300Z", datetime(2026, 10, 30, 9, 0, 0, 958410, tzinfo=timezone.utc)),
        ("2026-10-30T11:00:00.5+02:00", datetime(2026, 10, 30, 9, 0, 0, 500000, tzinfo=timezone.utc)),
        ("2026-10-30T09:00:00.123Z", datetime(2026, 10, 30, 9, 0, 0, 123000, tzinfo=timezone.utc)),
    ],
)
def test_provider_timestamps_parse_at_any_fraction_length(timestamp, expected):
    value = payload() | {"timestamp": timestamp}
    event = storage_event_from_message(QueueMessage("text", json.dumps(value), "receipt", 1))
    assert event.timestamp == expected


def test_optional_storage_metadata_can_be_absent():
    value = {key: payload()[key] for key in ("eventType", "bucketName", "objectKey", "timestamp")}
    event = storage_event_from_message(QueueMessage("text", json.dumps(value), "receipt", 1))
    assert event.size is None
    assert event.etag is None
    assert event.content_type is None
    assert event.metadata == {}
    assert event.copy_source is None
    assert event.previous_tier is None
    assert event.current_tier is None
    assert event.region is None
    assert event.version_id is None


@pytest.mark.parametrize(
    ("field", "invalid"),
    [
        ("eventType", "typo"),
        ("timestamp", "today"),
        ("timestamp", "2026-10-30T09:00:00"),
        ("timestamp", "2026-02-30T09:00:00Z"),
        ("size", -1),
        ("size", True),
        ("size", 1.5),
        ("size", 2**64),
        ("objectKey", 42),
        ("bucketName", None),
        ("metadata", {"key": 1}),
        ("metadata", None),
        ("etag", 42),
        ("versionId", []),
    ],
)
def test_invalid_storage_event_fields_fail(field, invalid):
    value = payload()
    value[field] = invalid
    with pytest.raises(AlienError) as caught:
        storage_event_from_message(QueueMessage("json", json.dumps(value), "unacknowledged", 1))
    error = caught.value
    assert error.code == "INVALID_STORAGE_EVENT_MESSAGE"
    assert error.retryable is False
    assert error.internal is False
    assert error.http_status_code == 400
    assert error.context["reason"]
    assert error.__cause__ is not None


@pytest.mark.parametrize("invalid", [b"\xff", b"not json", b"null", b"[]", b"{}"])
def test_invalid_worker_payloads_fail(invalid):
    with pytest.raises(AlienError) as caught:
        storage_event_from_message(QueueEvent("id", invalid, "jobs", 1))
    assert caught.value.code == "INVALID_STORAGE_EVENT_MESSAGE"
    assert caught.value.__cause__ is not None
