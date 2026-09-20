"""Generic smoke workload for every application-facing Python binding."""

from __future__ import annotations

import asyncio

from alienplatform import (
    ai,
    container,
    key,
    kv,
    postgres,
    queue,
    sandbox,
    storage,
    vault,
    worker,
)


async def main() -> None:
    files = storage("files")
    await files.put("smoke/payload", b"hello")
    assert await files.get("smoke/payload") == b"hello"

    cache = kv("cache")
    await cache.put("smoke", b"ready")
    assert (await cache.get("smoke")) is not None

    jobs = queue("jobs")
    await jobs.send({"kind": "smoke"})
    [message] = await jobs.receive()
    await jobs.ack(message.receipt_handle)

    secrets = vault("secrets")
    await secrets.put("smoke", "ready")
    assert await secrets.get("smoke") == "ready"

    ciphertext = await key("records").encrypt(b"private")
    assert await key("records").decrypt(ciphertext) == b"private"

    assert (await postgres("database").connection()).database
    assert await container("api").internal_url()
    assert (await ai("models").connection()).base_url

    response = await worker("processor").invoke(method="POST", path="/smoke")
    assert response.status < 500

    runtime = sandbox("runtime")
    instance, _ = await runtime.get_or_create(tenant_key="python-bindings-smoke")
    command = await runtime.run_command(
        instance.sandbox_id,
        "python",
        ["-c", "print('ready')"],
        timeout_ms=30_000,
    )
    async for frame in command:
        if frame.exit_code is not None:
            assert frame.exit_code == 0


if __name__ == "__main__":
    asyncio.run(main())
