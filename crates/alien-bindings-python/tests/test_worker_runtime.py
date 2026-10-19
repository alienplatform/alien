"""Exercise the generated wire protocol and typed application batch handler together."""

import asyncio

import grpc
import pytest
from alienplatform import worker_runtime
from alienplatform._worker_protocol import control_pb2 as protocol
from alienplatform._worker_protocol import control_pb2_grpc as service


@pytest.mark.parametrize("mode", ["partial", "invalid", "crash"])
async def test_batch_handler_results_over_real_grpc(mode):
    completed = asyncio.Event()
    results = []
    registrations = []

    class Runtime(service.ControlServiceServicer):
        async def RegisterEventHandler(self, request, context):
            registrations.append((request.handler_type, request.resource_name))
            return protocol.RegisterEventHandlerResponse(success=True)

        async def WaitForTasks(self, request, context):
            yield protocol.Task(
                task_id="batch-task",
                queue_batch=protocol.QueueBatch(
                    source="jobs",
                    deliveries=[
                        protocol.QueueDelivery(
                            delivery_id=identity,
                            message=protocol.QueueMessage(
                                id="same-provider-id",
                                source="jobs",
                                payload=b'{"job":1}',
                                attempt_count=2,
                            ),
                        )
                        for identity in ("attempt-a", "attempt-b")
                    ],
                ),
            )
            await completed.wait()

        async def SendTaskResult(self, request, context):
            results.append(request)
            completed.set()
            return protocol.SendTaskResultResponse(acknowledged=True)

    async def handler(batch):
        assert batch.source == "jobs"
        assert len(batch.deliveries) == 2
        assert batch.deliveries[0].message.id == batch.deliveries[1].message.id
        assert batch.deliveries[0].delivery_id != batch.deliveries[1].delivery_id
        assert batch.deliveries[0].message.attempt_count == 2
        if mode == "crash":
            raise ValueError("handler failed")
        if mode == "invalid":
            return [worker_runtime.QueueOutcome("attempt-a", "success")]
        return [
            worker_runtime.QueueOutcome("attempt-b", "retry"),
            worker_runtime.QueueOutcome("attempt-a", "success"),
        ]

    worker_runtime._batch_handlers.clear()
    worker_runtime._single_handlers.clear()
    worker_runtime.on_queue_batch("jobs", handler)
    server = grpc.aio.server()
    service.add_ControlServiceServicer_to_server(Runtime(), server)
    port = server.add_insecure_port("127.0.0.1:0")
    await server.start()
    try:
        await asyncio.wait_for(worker_runtime.run_worker(address=f"127.0.0.1:{port}"), timeout=5)
    finally:
        await server.stop(0)
        worker_runtime._batch_handlers.clear()
    assert registrations == [("queueBatch", "jobs")]
    assert len(results) == 1
    assert results[0].task_id == "batch-task"
    if mode == "partial":
        assert results[0].WhichOneof("result") == "queue_batch"
        assert [
            (item.delivery_id, item.disposition) for item in results[0].queue_batch.outcomes
        ] == [
            ("attempt-b", protocol.QueueDeliveryOutcome.RETRY),
            ("attempt-a", protocol.QueueDeliveryOutcome.SUCCESS),
        ]
    else:
        assert results[0].WhichOneof("result") == "error"
        assert results[0].error.code == (
            "BATCH_OUTCOMES_INVALID" if mode == "invalid" else "HANDLER_ERROR"
        )


async def test_cancelling_worker_cancels_active_handler_without_acknowledgment():
    entered = asyncio.Event()
    stopped = asyncio.Event()
    results = []

    class Runtime(service.ControlServiceServicer):
        async def RegisterEventHandler(self, request, context):
            return protocol.RegisterEventHandlerResponse(success=True)

        async def WaitForTasks(self, request, context):
            yield protocol.Task(
                task_id="active-task",
                queue_batch=protocol.QueueBatch(
                    source="jobs",
                    deliveries=[
                        protocol.QueueDelivery(
                            delivery_id="delivery",
                            message=protocol.QueueMessage(id="message", source="jobs"),
                        )
                    ],
                ),
            )
            await asyncio.Event().wait()

        async def SendTaskResult(self, request, context):
            results.append(request)
            return protocol.SendTaskResultResponse(acknowledged=True)

    async def handler(batch):
        assert batch.source == "jobs"
        entered.set()
        try:
            await asyncio.Event().wait()
        finally:
            stopped.set()

    worker_runtime._batch_handlers.clear()
    worker_runtime._single_handlers.clear()
    worker_runtime.on_queue_batch("*", handler)
    server = grpc.aio.server()
    service.add_ControlServiceServicer_to_server(Runtime(), server)
    port = server.add_insecure_port("127.0.0.1:0")
    await server.start()
    worker = asyncio.create_task(worker_runtime.run_worker(address=f"127.0.0.1:{port}"))
    try:
        await asyncio.wait_for(entered.wait(), timeout=5)
        worker.cancel()
        with pytest.raises(asyncio.CancelledError):
            await asyncio.wait_for(worker, timeout=5)
        assert stopped.is_set()
        assert results == []
    finally:
        if not worker.done():
            worker.cancel()
            with pytest.raises(asyncio.CancelledError):
                await worker
        await server.stop(0)
        worker_runtime._batch_handlers.clear()
