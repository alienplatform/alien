import datetime

from google.protobuf import timestamp_pb2 as _timestamp_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf.internal import enum_type_wrapper as _enum_type_wrapper
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class RegisterHttpServerRequest(_message.Message):
    __slots__ = ("port",)
    PORT_FIELD_NUMBER: _ClassVar[int]
    port: int
    def __init__(self, port: _Optional[int] = ...) -> None: ...

class RegisterHttpServerResponse(_message.Message):
    __slots__ = ("success",)
    SUCCESS_FIELD_NUMBER: _ClassVar[int]
    success: bool
    def __init__(self, success: _Optional[bool] = ...) -> None: ...

class RegisterEventHandlerRequest(_message.Message):
    __slots__ = ("handler_type", "resource_name")
    HANDLER_TYPE_FIELD_NUMBER: _ClassVar[int]
    RESOURCE_NAME_FIELD_NUMBER: _ClassVar[int]
    handler_type: str
    resource_name: str
    def __init__(self, handler_type: _Optional[str] = ..., resource_name: _Optional[str] = ...) -> None: ...

class RegisterEventHandlerResponse(_message.Message):
    __slots__ = ("success",)
    SUCCESS_FIELD_NUMBER: _ClassVar[int]
    success: bool
    def __init__(self, success: _Optional[bool] = ...) -> None: ...

class WaitForTasksRequest(_message.Message):
    __slots__ = ("application_id",)
    APPLICATION_ID_FIELD_NUMBER: _ClassVar[int]
    application_id: str
    def __init__(self, application_id: _Optional[str] = ...) -> None: ...

class Task(_message.Message):
    __slots__ = ("task_id", "storage_event", "cron_event", "queue_message", "arc_command", "queue_batch")
    TASK_ID_FIELD_NUMBER: _ClassVar[int]
    STORAGE_EVENT_FIELD_NUMBER: _ClassVar[int]
    CRON_EVENT_FIELD_NUMBER: _ClassVar[int]
    QUEUE_MESSAGE_FIELD_NUMBER: _ClassVar[int]
    ARC_COMMAND_FIELD_NUMBER: _ClassVar[int]
    QUEUE_BATCH_FIELD_NUMBER: _ClassVar[int]
    task_id: str
    storage_event: StorageEvent
    cron_event: CronEvent
    queue_message: QueueMessage
    arc_command: ArcCommand
    queue_batch: QueueBatch
    def __init__(self, task_id: _Optional[str] = ..., storage_event: _Optional[_Union[StorageEvent, _Mapping]] = ..., cron_event: _Optional[_Union[CronEvent, _Mapping]] = ..., queue_message: _Optional[_Union[QueueMessage, _Mapping]] = ..., arc_command: _Optional[_Union[ArcCommand, _Mapping]] = ..., queue_batch: _Optional[_Union[QueueBatch, _Mapping]] = ...) -> None: ...

class StorageEvent(_message.Message):
    __slots__ = ("key", "event_type", "bucket", "size", "content_type", "timestamp", "etag", "region", "version_id", "current_tier", "metadata")
    class MetadataEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    KEY_FIELD_NUMBER: _ClassVar[int]
    EVENT_TYPE_FIELD_NUMBER: _ClassVar[int]
    BUCKET_FIELD_NUMBER: _ClassVar[int]
    SIZE_FIELD_NUMBER: _ClassVar[int]
    CONTENT_TYPE_FIELD_NUMBER: _ClassVar[int]
    TIMESTAMP_FIELD_NUMBER: _ClassVar[int]
    ETAG_FIELD_NUMBER: _ClassVar[int]
    REGION_FIELD_NUMBER: _ClassVar[int]
    VERSION_ID_FIELD_NUMBER: _ClassVar[int]
    CURRENT_TIER_FIELD_NUMBER: _ClassVar[int]
    METADATA_FIELD_NUMBER: _ClassVar[int]
    key: str
    event_type: str
    bucket: str
    size: int
    content_type: str
    timestamp: _timestamp_pb2.Timestamp
    etag: str
    region: str
    version_id: str
    current_tier: str
    metadata: _containers.ScalarMap[str, str]
    def __init__(self, key: _Optional[str] = ..., event_type: _Optional[str] = ..., bucket: _Optional[str] = ..., size: _Optional[int] = ..., content_type: _Optional[str] = ..., timestamp: _Optional[_Union[datetime.datetime, _timestamp_pb2.Timestamp, _Mapping]] = ..., etag: _Optional[str] = ..., region: _Optional[str] = ..., version_id: _Optional[str] = ..., current_tier: _Optional[str] = ..., metadata: _Optional[_Mapping[str, str]] = ...) -> None: ...

class CronEvent(_message.Message):
    __slots__ = ("schedule_name", "scheduled_time")
    SCHEDULE_NAME_FIELD_NUMBER: _ClassVar[int]
    SCHEDULED_TIME_FIELD_NUMBER: _ClassVar[int]
    schedule_name: str
    scheduled_time: _timestamp_pb2.Timestamp
    def __init__(self, schedule_name: _Optional[str] = ..., scheduled_time: _Optional[_Union[datetime.datetime, _timestamp_pb2.Timestamp, _Mapping]] = ...) -> None: ...

class QueueMessage(_message.Message):
    __slots__ = ("id", "payload", "receipt_handle", "source", "attempt_count", "timestamp", "attributes")
    class AttributesEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    ID_FIELD_NUMBER: _ClassVar[int]
    PAYLOAD_FIELD_NUMBER: _ClassVar[int]
    RECEIPT_HANDLE_FIELD_NUMBER: _ClassVar[int]
    SOURCE_FIELD_NUMBER: _ClassVar[int]
    ATTEMPT_COUNT_FIELD_NUMBER: _ClassVar[int]
    TIMESTAMP_FIELD_NUMBER: _ClassVar[int]
    ATTRIBUTES_FIELD_NUMBER: _ClassVar[int]
    id: str
    payload: bytes
    receipt_handle: str
    source: str
    attempt_count: int
    timestamp: _timestamp_pb2.Timestamp
    attributes: _containers.ScalarMap[str, str]
    def __init__(self, id: _Optional[str] = ..., payload: _Optional[bytes] = ..., receipt_handle: _Optional[str] = ..., source: _Optional[str] = ..., attempt_count: _Optional[int] = ..., timestamp: _Optional[_Union[datetime.datetime, _timestamp_pb2.Timestamp, _Mapping]] = ..., attributes: _Optional[_Mapping[str, str]] = ...) -> None: ...

class ArcCommand(_message.Message):
    __slots__ = ("command_id", "command_name", "params", "attempt", "deadline", "response_url", "storage_upload_url", "max_inline_bytes")
    COMMAND_ID_FIELD_NUMBER: _ClassVar[int]
    COMMAND_NAME_FIELD_NUMBER: _ClassVar[int]
    PARAMS_FIELD_NUMBER: _ClassVar[int]
    ATTEMPT_FIELD_NUMBER: _ClassVar[int]
    DEADLINE_FIELD_NUMBER: _ClassVar[int]
    RESPONSE_URL_FIELD_NUMBER: _ClassVar[int]
    STORAGE_UPLOAD_URL_FIELD_NUMBER: _ClassVar[int]
    MAX_INLINE_BYTES_FIELD_NUMBER: _ClassVar[int]
    command_id: str
    command_name: str
    params: bytes
    attempt: int
    deadline: _timestamp_pb2.Timestamp
    response_url: str
    storage_upload_url: str
    max_inline_bytes: int
    def __init__(self, command_id: _Optional[str] = ..., command_name: _Optional[str] = ..., params: _Optional[bytes] = ..., attempt: _Optional[int] = ..., deadline: _Optional[_Union[datetime.datetime, _timestamp_pb2.Timestamp, _Mapping]] = ..., response_url: _Optional[str] = ..., storage_upload_url: _Optional[str] = ..., max_inline_bytes: _Optional[int] = ...) -> None: ...

class SendTaskResultRequest(_message.Message):
    __slots__ = ("task_id", "success", "error", "queue_batch")
    TASK_ID_FIELD_NUMBER: _ClassVar[int]
    SUCCESS_FIELD_NUMBER: _ClassVar[int]
    ERROR_FIELD_NUMBER: _ClassVar[int]
    QUEUE_BATCH_FIELD_NUMBER: _ClassVar[int]
    task_id: str
    success: TaskSuccess
    error: TaskError
    queue_batch: QueueBatchResult
    def __init__(self, task_id: _Optional[str] = ..., success: _Optional[_Union[TaskSuccess, _Mapping]] = ..., error: _Optional[_Union[TaskError, _Mapping]] = ..., queue_batch: _Optional[_Union[QueueBatchResult, _Mapping]] = ...) -> None: ...

class TaskSuccess(_message.Message):
    __slots__ = ("response_data",)
    RESPONSE_DATA_FIELD_NUMBER: _ClassVar[int]
    response_data: bytes
    def __init__(self, response_data: _Optional[bytes] = ...) -> None: ...

class TaskError(_message.Message):
    __slots__ = ("code", "message")
    CODE_FIELD_NUMBER: _ClassVar[int]
    MESSAGE_FIELD_NUMBER: _ClassVar[int]
    code: str
    message: str
    def __init__(self, code: _Optional[str] = ..., message: _Optional[str] = ...) -> None: ...

class SendTaskResultResponse(_message.Message):
    __slots__ = ("acknowledged",)
    ACKNOWLEDGED_FIELD_NUMBER: _ClassVar[int]
    acknowledged: bool
    def __init__(self, acknowledged: _Optional[bool] = ...) -> None: ...

class QueueDelivery(_message.Message):
    __slots__ = ("delivery_id", "message")
    DELIVERY_ID_FIELD_NUMBER: _ClassVar[int]
    MESSAGE_FIELD_NUMBER: _ClassVar[int]
    delivery_id: str
    message: QueueMessage
    def __init__(self, delivery_id: _Optional[str] = ..., message: _Optional[_Union[QueueMessage, _Mapping]] = ...) -> None: ...

class QueueBatch(_message.Message):
    __slots__ = ("source", "deliveries")
    SOURCE_FIELD_NUMBER: _ClassVar[int]
    DELIVERIES_FIELD_NUMBER: _ClassVar[int]
    source: str
    deliveries: _containers.RepeatedCompositeFieldContainer[QueueDelivery]
    def __init__(self, source: _Optional[str] = ..., deliveries: _Optional[_Iterable[_Union[QueueDelivery, _Mapping]]] = ...) -> None: ...

class QueueDeliveryOutcome(_message.Message):
    __slots__ = ("delivery_id", "disposition")
    class Disposition(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
        __slots__ = ()
        UNSPECIFIED: _ClassVar[QueueDeliveryOutcome.Disposition]
        SUCCESS: _ClassVar[QueueDeliveryOutcome.Disposition]
        RETRY: _ClassVar[QueueDeliveryOutcome.Disposition]
    UNSPECIFIED: QueueDeliveryOutcome.Disposition
    SUCCESS: QueueDeliveryOutcome.Disposition
    RETRY: QueueDeliveryOutcome.Disposition
    DELIVERY_ID_FIELD_NUMBER: _ClassVar[int]
    DISPOSITION_FIELD_NUMBER: _ClassVar[int]
    delivery_id: str
    disposition: QueueDeliveryOutcome.Disposition
    def __init__(self, delivery_id: _Optional[str] = ..., disposition: _Optional[_Union[QueueDeliveryOutcome.Disposition, str]] = ...) -> None: ...

class QueueBatchResult(_message.Message):
    __slots__ = ("outcomes",)
    OUTCOMES_FIELD_NUMBER: _ClassVar[int]
    outcomes: _containers.RepeatedCompositeFieldContainer[QueueDeliveryOutcome]
    def __init__(self, outcomes: _Optional[_Iterable[_Union[QueueDeliveryOutcome, _Mapping]]] = ...) -> None: ...
