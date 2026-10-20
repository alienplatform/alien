"""Typed queue handlers for Worker applications (install alienplatform[worker])."""

from __future__ import annotations

import asyncio
import os
import uuid
from collections.abc import Awaitable, Callable, Sequence
from dataclasses import dataclass
from typing import Literal

from .errors import AlienError


@dataclass(frozen=True)
class QueueEvent:
    """One provider message; its ID is not a delivery-attempt identity."""

    id: str
    payload: bytes
    source: str
    attempt_count: int


@dataclass(frozen=True)
class QueueDelivery:
    """A distinct delivery attempt within a batch."""

    delivery_id: str
    message: QueueEvent


@dataclass(frozen=True)
class QueueBatch:
    """An unordered, possibly undersized batch."""

    source: str
    deliveries: tuple[QueueDelivery, ...]


@dataclass(frozen=True)
class QueueOutcome:
    """Return exactly one success or retry outcome for every delivery ID."""

    delivery_id: str
    disposition: Literal["success", "retry"]


_single_handlers: dict[str, Callable[[QueueEvent], Awaitable[None]]] = {}
_batch_handlers: dict[str, Callable[[QueueBatch], Awaitable[Sequence[QueueOutcome]]]] = {}


def on_queue_message(source: str, handler: Callable[[QueueEvent], Awaitable[None]]) -> None:
    """Register a single-message handler before calling run_worker."""
    _single_handlers[source] = handler


def on_queue_batch(
    source: str, handler: Callable[[QueueBatch], Awaitable[Sequence[QueueOutcome]]]
) -> None:
    """Register a batch handler for an explicitly configured batch trigger."""
    _batch_handlers[source] = handler


async def run_worker(*, address: str | None = None) -> None:
    """Run queue handlers over the runtime protocol until shutdown or connection loss.

    Handlers execute sequentially. The process host terminates the application
    on runtime cancellation, including tasks that do not cooperate with asyncio.
    Protocol failure exits this loop; it never acknowledges uncompleted work.
    """
    import grpc

    from ._worker_protocol import control_pb2 as protocol
    from ._worker_protocol.control_pb2_grpc import ControlServiceStub

    endpoint = address or os.environ.get("ALIEN_WORKER_GRPC_ADDRESS")
    if not endpoint:
        raise AlienError("Worker protocol address is required", code="WORKER_ADDRESS_MISSING")
    endpoint = endpoint.removeprefix("http://")
    async with grpc.aio.insecure_channel(
        endpoint, options=[("grpc.max_receive_message_length", 4 * 1024 * 1024)]
    ) as channel:
        client = ControlServiceStub(channel)
        for kind, handlers in (("queue", _single_handlers), ("queueBatch", _batch_handlers)):
            for source in handlers:
                response = await client.RegisterEventHandler(
                    protocol.RegisterEventHandlerRequest(handler_type=kind, resource_name=source),
                    timeout=30,
                )
                if not response.success:
                    raise AlienError("Handler registration rejected", code="HANDLER_REJECTED")
        try:
            async for task in client.WaitForTasks(
                protocol.WaitForTasksRequest(application_id=str(uuid.uuid4()))
            ):
                result = protocol.SendTaskResultRequest(task_id=task.task_id)
                try:
                    kind = task.WhichOneof("payload")
                    if kind == "queue_batch":
                        batch = task.queue_batch
                        handler = _batch_handlers.get(batch.source)
                        if handler is None:
                            handler = _batch_handlers.get("*")
                        if handler is None:
                            raise AlienError("No batch handler", code="HANDLER_NOT_FOUND")
                        deliveries = tuple(
                            QueueDelivery(
                                delivery.delivery_id,
                                QueueEvent(
                                    delivery.message.id,
                                    delivery.message.payload,
                                    delivery.message.source,
                                    delivery.message.attempt_count,
                                ),
                            )
                            for delivery in batch.deliveries
                        )
                        outcomes = await handler(QueueBatch(batch.source, deliveries))
                        expected = {delivery.delivery_id for delivery in deliveries}
                        if (
                            len(outcomes) != len(deliveries)
                            or {outcome.delivery_id for outcome in outcomes} != expected
                            or any(
                                outcome.disposition not in ("success", "retry")
                                for outcome in outcomes
                            )
                        ):
                            raise AlienError(
                                "Invalid batch outcomes", code="BATCH_OUTCOMES_INVALID"
                            )
                        result.queue_batch.CopyFrom(
                            protocol.QueueBatchResult(
                                outcomes=[
                                    protocol.QueueDeliveryOutcome(
                                        delivery_id=outcome.delivery_id,
                                        disposition=(
                                            protocol.QueueDeliveryOutcome.SUCCESS
                                            if outcome.disposition == "success"
                                            else protocol.QueueDeliveryOutcome.RETRY
                                        ),
                                    )
                                    for outcome in outcomes
                                ]
                            )
                        )
                    elif kind == "queue_message":
                        message = task.queue_message
                        single = _single_handlers.get(message.source)
                        if single is None:
                            single = _single_handlers.get("*")
                        if single is None:
                            raise AlienError("No queue handler", code="HANDLER_NOT_FOUND")
                        await single(
                            QueueEvent(
                                message.id, message.payload, message.source, message.attempt_count
                            )
                        )
                        result.success.CopyFrom(protocol.TaskSuccess())
                    else:
                        raise AlienError("Unsupported Worker task", code="HANDLER_NOT_FOUND")
                except asyncio.CancelledError:
                    raise
                except Exception as error:
                    result.error.CopyFrom(
                        protocol.TaskError(
                            code=getattr(error, "code", "HANDLER_ERROR"), message=str(error)
                        )
                    )
                response = await client.SendTaskResult(result, timeout=30)
                if not response.acknowledged:
                    raise AlienError("Task result rejected", code="TASK_RESULT_REJECTED")
        except grpc.aio.AioRpcError as error:
            raise AlienError(
                "Worker protocol connection failed", code="WORKER_CONNECTION_FAILED"
            ) from error
