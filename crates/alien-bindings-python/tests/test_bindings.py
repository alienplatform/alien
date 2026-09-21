from __future__ import annotations

import asyncio
import gc
import json
import os
import ssl
import threading
import weakref
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from types import SimpleNamespace

import alienplatform.bindings as bindings_module
import pytest
from alienplatform import (
    AlienError,
    ai,
    container,
    kv,
    postgres,
    queue,
    sandbox,
    storage,
    vault,
    worker,
)
from alienplatform.bindings import AiConnection, PostgresConnection


def bind(name: str, value: dict[str, object]) -> None:
    os.environ["ALIEN_DEPLOYMENT_TYPE"] = "local"
    os.environ[f"ALIEN_{name.replace('-', '_').upper()}_BINDING"] = json.dumps(value)


@pytest.mark.asyncio
async def test_storage_round_trip_and_structured_missing_error(tmp_path: Path) -> None:
    bind("files", {"service": "local-storage", "storagePath": str(tmp_path / "storage")})
    files = storage("files")
    payload = bytes([0, 1, 255, 0, 128])
    await files.put("nested/data.bin", payload)
    assert await files.get("nested/data.bin") == payload
    assert [item.location for item in await files.list("nested")] == ["nested/data.bin"]
    await files.delete("nested/data.bin")
    with pytest.raises(AlienError) as raised:
        await files.get("nested/data.bin")
    assert raised.value.code == "STORAGE_OBJECT_NOT_FOUND"
    assert "data.bin" not in str(raised.value)


@pytest.mark.asyncio
async def test_kv_queue_and_vault_real_local_providers(tmp_path: Path) -> None:
    bind("cache", {"service": "local-kv", "dataDir": str(tmp_path / "kv")})
    bind("jobs", {"service": "local-queue", "queuePath": str(tmp_path / "queue")})
    bind(
        "secrets",
        {"service": "local-vault", "vaultName": "test", "dataDir": str(tmp_path / "vault")},
    )

    cache = kv("cache")
    assert await cache.put("key", b"value", condition="absent") is True
    entry = await cache.get("key")
    assert entry is not None and bytes(entry.value) == b"value"
    assert await cache.put("key", b"other", condition="absent") is False

    jobs = queue("jobs")
    await jobs.send({"job": 7})
    [message] = await jobs.receive()
    assert message.payload_type == "json"
    assert json.loads(message.payload) == {"job": 7}
    await jobs.ack(message.receipt_handle)
    assert await jobs.receive() == []

    secrets = vault("secrets")
    await secrets.put("credential", "value")
    assert await secrets.get("credential") == "value"
    assert await secrets.list() == ["credential"]
    await secrets.delete("credential")


@pytest.mark.asyncio
async def test_postgres_helpers_and_container_discovery() -> None:
    bind(
        "database",
        {
            "service": "local-postgres",
            "host": "db.internal",
            "port": 5432,
            "database": "app",
            "username": "alien",
            "password": "p@ss/word",
        },
    )
    bind(
        "api",
        {
            "service": "local",
            "containerName": "api",
            "internalUrl": "http://api.svc:8000",
            "publicUrl": "http://localhost:18000",
        },
    )

    connection = await postgres("database").connection()
    assert (
        connection.connection_string
        == "postgres://alien:p%40ss%2Fword@db.internal:5432/app?sslmode=disable"
    )
    assert (
        connection.sqlalchemy_async_url()
        == "postgresql+asyncpg://alien:p%40ss%2Fword@db.internal:5432/app?ssl=disable"
    )
    assert connection.ssl_context() is None
    assert "p@ss/word" not in repr(connection)
    assert await container("api").internal_url() == "http://api.svc:8000"
    assert await container("api").public_url() == "http://localhost:18000"


def test_postgres_sqlalchemy_helper_preserves_tls_verification(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class FakeContext:
        def __init__(self) -> None:
            self.loaded: list[str] = []
            self.check_hostname = True

        def load_verify_locations(self, *, cadata: str) -> None:
            self.loaded.append(cadata)

    context = FakeContext()
    monkeypatch.setattr(ssl, "create_default_context", lambda: context)
    connection = PostgresConnection(
        "postgres://user:password@db.internal/app?sslmode=verify-full",
        "db.internal",
        5432,
        "app",
        "user",
        "password",
        "verify-full",
        ("test-ca",),
    )

    kwargs = connection.sqlalchemy_async_engine_kwargs()
    assert kwargs["url"].endswith("?ssl=verify-full")
    assert kwargs["connect_args"] == {"ssl": context}
    assert context.loaded == ["test-ca"]
    assert context.check_hostname is True

    disabled = PostgresConnection(
        "postgres://user:password@localhost/app?sslmode=disable",
        "localhost",
        5432,
        "app",
        "user",
        "password",
        "disable",
        (),
    )
    assert disabled.sqlalchemy_async_engine_kwargs()["connect_args"] == {"ssl": False}


@pytest.mark.asyncio
async def test_cancelling_one_waiter_does_not_cancel_shared_initialization(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    started = asyncio.Event()
    release = asyncio.Event()
    resolved_handle = object()
    calls = 0

    class FakeBindingsHandle:
        async def storage(self, name: str) -> object:
            nonlocal calls
            assert name == "files"
            calls += 1
            started.set()
            await release.wait()
            return resolved_handle

    monkeypatch.setattr(bindings_module._native, "BindingsHandle", FakeBindingsHandle)
    lazy = bindings_module._LazyHandle[object]("storage", "files")
    cancelled_waiter = asyncio.create_task(lazy.get())
    await started.wait()
    surviving_waiter = asyncio.create_task(lazy.get())

    cancelled_waiter.cancel()
    with pytest.raises(asyncio.CancelledError):
        await cancelled_waiter
    release.set()

    assert await surviving_waiter is resolved_handle
    assert await lazy.get() is resolved_handle
    assert calls == 1


@pytest.mark.asyncio
async def test_missing_binding_is_stable_alien_error() -> None:
    with pytest.raises(AlienError) as raised:
        await storage("missing-resource").list()
    assert raised.value.code == "BINDING_NOT_CONFIGURED"
    assert raised.value.retryable is False
    assert raised.value.context == {
        "binding_name": "missing-resource",
        "env_var": "ALIEN_MISSING_RESOURCE_BINDING",
    }

    with pytest.raises(AlienError) as sandbox_error:
        await sandbox("missing-sandbox").capabilities()
    assert sandbox_error.value.code == "BINDING_NOT_CONFIGURED"


@pytest.mark.asyncio
async def test_external_ai_connection_is_typed_and_redacted(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    bind(
        "model",
        {"service": "external-ai", "provider": "openai", "apiKey": "test-secret"},
    )
    monkeypatch.setenv("ALIEN_AI_LOCAL_BASE_URL", "http://localhost:9876/")

    connection = await ai("model").connection()
    assert connection.base_url == "http://localhost:9876"
    assert connection.openai_base_url == "http://localhost:9876/v1"
    assert connection.api_key == "test-secret"
    assert connection.provider == "openai"
    assert "test-secret" not in repr(connection)


@pytest.mark.asyncio
async def test_ai_connection_retains_managed_gateway_owner(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class FakeAiHandle:
        def connection(self) -> SimpleNamespace:
            return SimpleNamespace(
                base_url="http://127.0.0.1:1234",
                api_key="secret",
                provider="managed",
            )

    handle = FakeAiHandle()
    handle_ref = weakref.ref(handle)
    handles = {"assistant": handle}

    class FakeBindingsHandle:
        async def ai(self, name: str) -> FakeAiHandle:
            assert name == "assistant"
            return handles[name]

    monkeypatch.setattr(bindings_module._native, "BindingsHandle", FakeBindingsHandle)
    resource = bindings_module.Ai("assistant")
    connection = await resource.connection()
    assert connection._owner is handle

    # Remove the resource factory and its completed lazy-resolution task. The connection must
    # independently keep the native gateway alive for as long as callers retain it.
    del resource
    del handle
    handles.clear()
    await asyncio.sleep(0)
    gc.collect()
    assert handle_ref() is not None
    assert connection == AiConnection("http://127.0.0.1:1234", "secret", "managed")
    assert "secret" not in repr(connection)

    del connection
    await asyncio.sleep(0)
    gc.collect()
    assert handle_ref() is None


@pytest.mark.asyncio
async def test_worker_invocation_and_public_url() -> None:
    class Handler(BaseHTTPRequestHandler):
        def do_POST(self) -> None:
            assert self.path == "/jobs"
            body = self.rfile.read(int(self.headers["Content-Length"]))
            assert body == b"payload"
            self.send_response(202)
            self.send_header("X-Worker", "local")
            self.end_headers()
            self.wfile.write(b"accepted")

        def log_message(self, format: str, *args: object) -> None:
            del format, args

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        url = f"http://127.0.0.1:{server.server_port}"
        bind("processor", {"service": "local", "workerUrl": url})
        processor = worker("processor")
        response = await processor.invoke(method="POST", path="/jobs", body=b"payload")
        assert response.status == 202
        assert response.headers["x-worker"] == "local"
        assert response.body == b"accepted"
        assert await processor.public_url() == url
    finally:
        server.shutdown()
        thread.join()
