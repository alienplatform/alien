from __future__ import annotations

import json
import os
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

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
