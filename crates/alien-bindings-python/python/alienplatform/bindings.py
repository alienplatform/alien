"""Lazy, typed factories for resources linked to the current workload."""

from __future__ import annotations

import asyncio
import json
import ssl
from collections.abc import AsyncIterator, Mapping, Sequence
from dataclasses import dataclass
from typing import Any, Generic, Literal, TypeVar
from urllib.parse import parse_qsl, urlencode, urlsplit, urlunsplit

from . import _native  # type: ignore[attr-defined]
from .errors import translate_errors

H = TypeVar("H")


class _LazyHandle(Generic[H]):
    def __init__(self, kind: str, name: str) -> None:
        self._kind = kind
        self._name = name
        self._task: asyncio.Task[H] | None = None

    async def get(self) -> H:
        if self._task is None:

            async def resolve() -> H:
                bindings = _native.BindingsHandle()
                return await getattr(bindings, self._kind)(self._name)

            self._task = asyncio.create_task(resolve())
        try:
            return await self._task
        except Exception:
            self._task = None
            raise


@dataclass(frozen=True, repr=False)
class PostgresConnection:
    connection_string: str
    host: str
    port: int
    database: str
    username: str
    password: str
    sslmode: Literal["disable", "verify-ca", "verify-full"]
    ca_certificates: tuple[str, ...]

    def __repr__(self) -> str:
        return (
            "PostgresConnection("
            f"host={self.host!r}, port={self.port!r}, database={self.database!r}, "
            f"username={self.username!r}, password='<redacted>', "
            f"sslmode={self.sslmode!r}, ca_certificates={len(self.ca_certificates)})"
        )

    def sqlalchemy_async_url(self) -> str:
        """Return a SQLAlchemy asyncpg URL without leaking driver translation to apps."""
        parts = urlsplit(self.connection_string)
        query = dict(parse_qsl(parts.query, keep_blank_values=True))
        sslmode = query.pop("sslmode", self.sslmode)
        query["ssl"] = "disable" if sslmode == "disable" else "require"
        return urlunsplit(
            ("postgresql+asyncpg", parts.netloc, parts.path, urlencode(query), parts.fragment)
        )

    def ssl_context(self) -> ssl.SSLContext | None:
        """Build a TLS context for drivers that accept structured SSL configuration."""
        if self.sslmode == "disable":
            return None
        context = ssl.create_default_context()
        for certificate in self.ca_certificates:
            context.load_verify_locations(cadata=certificate)
        context.check_hostname = self.sslmode == "verify-full"
        return context


class Storage:
    def __init__(self, name: str) -> None:
        self._handle = _LazyHandle[Any]("storage", name)

    @translate_errors
    async def get(self, path: str) -> bytes:
        return bytes(await (await self._handle.get()).get(path))

    @translate_errors
    async def put(self, path: str, data: bytes) -> None:
        await (await self._handle.get()).put(path, list(data))

    @translate_errors
    async def delete(self, path: str) -> None:
        await (await self._handle.get()).delete(path)

    @translate_errors
    async def list(self, prefix: str | None = None) -> list[ObjectInfo]:
        return [_object_info(value) for value in await (await self._handle.get()).list(prefix)]

    @translate_errors
    async def signed_url(
        self, method: Literal["GET", "PUT", "DELETE"], path: str, expires_in: int
    ) -> SignedRequest:
        value = await (await self._handle.get()).signed_url(method, path, expires_in)
        return SignedRequest(value.url, value.method, dict(value.headers))


@dataclass(frozen=True)
class ObjectInfo:
    location: str
    size: int
    last_modified: str
    e_tag: str | None
    version: str | None


@dataclass(frozen=True)
class SignedRequest:
    url: str
    method: str
    headers: Mapping[str, str]


def _object_info(value: Any) -> ObjectInfo:
    return ObjectInfo(
        value.location,
        value.size,
        value.last_modified,
        value.e_tag,
        value.version,
    )


class Key:
    def __init__(self, name: str) -> None:
        self._handle = _LazyHandle[Any]("key", name)

    @translate_errors
    async def encrypt(self, plaintext: bytes, context: Mapping[str, str] | None = None) -> bytes:
        return bytes(
            await (await self._handle.get()).encrypt(
                list(plaintext), dict(context) if context else None
            )
        )

    @translate_errors
    async def decrypt(self, ciphertext: bytes, context: Mapping[str, str] | None = None) -> bytes:
        return bytes(
            await (await self._handle.get()).decrypt(
                list(ciphertext), dict(context) if context else None
            )
        )


class Kv:
    def __init__(self, name: str) -> None:
        self._handle = _LazyHandle[Any]("kv", name)

    @translate_errors
    async def get(self, key: str) -> KvEntry | None:
        value = await (await self._handle.get()).get(key)
        return _kv_entry(value) if value is not None else None

    @translate_errors
    async def put(
        self,
        key: str,
        value: bytes,
        *,
        ttl_seconds: int | None = None,
        condition: Literal["absent", "version"] | None = None,
        version: str | None = None,
    ) -> bool:
        return await (await self._handle.get()).put(
            key, list(value), ttl_seconds, condition, version
        )

    @translate_errors
    async def delete(self, key: str, *, if_version: str | None = None) -> bool:
        return await (await self._handle.get()).delete(key, if_version)

    @translate_errors
    async def exists(self, key: str) -> bool:
        return await (await self._handle.get()).exists(key)

    @translate_errors
    async def scan(
        self, prefix: str, *, limit: int | None = None, cursor: str | None = None
    ) -> KvPage:
        value = await (await self._handle.get()).scan(prefix, limit, cursor)
        return KvPage(tuple(_kv_entry(item) for item in value.items), value.next_cursor)


@dataclass(frozen=True)
class KvEntry:
    key: str
    value: bytes
    version: str


@dataclass(frozen=True)
class KvPage:
    items: tuple[KvEntry, ...]
    next_cursor: str | None


def _kv_entry(value: Any) -> KvEntry:
    return KvEntry(value.key, bytes(value.value), value.version)


class Queue:
    def __init__(self, name: str) -> None:
        self._handle = _LazyHandle[Any]("queue", name)

    @translate_errors
    async def send(self, message: Any) -> None:
        await (await self._handle.get()).send_json(json.dumps(message, separators=(",", ":")))

    @translate_errors
    async def send_text(self, message: str) -> None:
        await (await self._handle.get()).send_text(message)

    @translate_errors
    async def receive(self, max_messages: int = 1) -> list[QueueMessage]:
        return [
            QueueMessage(value.payload_type, value.payload, value.receipt_handle, value.attempt)
            for value in await (await self._handle.get()).receive(max_messages)
        ]

    @translate_errors
    async def ack(self, receipt_handle: str) -> None:
        await (await self._handle.get()).ack(receipt_handle)

    @translate_errors
    async def nack(self, receipt_handle: str) -> None:
        await (await self._handle.get()).nack(receipt_handle)

    @translate_errors
    async def purge(self) -> None:
        await (await self._handle.get()).purge()


@dataclass(frozen=True)
class QueueMessage:
    payload_type: Literal["json", "text"]
    payload: str
    receipt_handle: str
    attempt: int


class Vault:
    def __init__(self, name: str) -> None:
        self._handle = _LazyHandle[Any]("vault", name)

    @translate_errors
    async def get(self, name: str) -> str:
        return await (await self._handle.get()).get(name)

    @translate_errors
    async def put(self, name: str, value: str) -> None:
        await (await self._handle.get()).put(name, value)

    @translate_errors
    async def delete(self, name: str) -> None:
        await (await self._handle.get()).delete(name)

    @translate_errors
    async def list(self) -> list[str]:
        return await (await self._handle.get()).list()


class Postgres:
    def __init__(self, name: str) -> None:
        self._handle = _LazyHandle[Any]("postgres", name)

    @translate_errors
    async def connection(self) -> PostgresConnection:
        value = (await self._handle.get()).connection()
        return PostgresConnection(
            value.connection_string,
            value.host,
            value.port,
            value.database,
            value.username,
            value.password,
            value.sslmode,
            tuple(value.ca_certificates),
        )


class Container:
    def __init__(self, name: str) -> None:
        self._handle = _LazyHandle[Any]("container", name)

    @translate_errors
    async def internal_url(self) -> str:
        return (await self._handle.get()).internal_url()

    @translate_errors
    async def public_url(self) -> str | None:
        return (await self._handle.get()).public_url()


@dataclass(frozen=True)
class WorkerResponse:
    status: int
    headers: Mapping[str, str]
    body: bytes


class Worker:
    def __init__(self, name: str) -> None:
        self._handle = _LazyHandle[Any]("worker", name)

    @translate_errors
    async def invoke(
        self,
        *,
        method: str,
        path: str,
        headers: Mapping[str, str] | None = None,
        body: bytes = b"",
        timeout_ms: int | None = None,
    ) -> WorkerResponse:
        value = await (await self._handle.get()).invoke(
            method,
            path,
            dict(headers or {}),
            list(body),
            timeout_ms,
        )
        return WorkerResponse(value.status, dict(value.headers), bytes(value.body))

    @translate_errors
    async def public_url(self) -> str | None:
        return await (await self._handle.get()).public_url()


@dataclass(frozen=True, repr=False)
class AiConnection:
    base_url: str
    api_key: str | None
    provider: str | None

    def __repr__(self) -> str:
        return (
            "AiConnection("
            f"base_url={self.base_url!r}, api_key='<redacted>', provider={self.provider!r})"
        )

    @property
    def openai_base_url(self) -> str:
        """Return the base URL expected by OpenAI-compatible clients."""
        return f"{self.base_url.rstrip('/')}/v1"


class Ai:
    def __init__(self, name: str) -> None:
        self._handle = _LazyHandle[Any]("ai", name)

    @translate_errors
    async def connection(self) -> AiConnection:
        value = (await self._handle.get()).connection()
        return AiConnection(value.base_url, value.api_key, value.provider)


@dataclass(frozen=True)
class SandboxInfo:
    sandbox_id: str
    state: Literal["starting", "running", "paused", "terminated"]
    generation: int


@dataclass(frozen=True)
class CommandFrame:
    kind: Literal["stdout", "stderr", "exit"]
    seq: int | None
    data: bytes | None
    exit_code: int | None
    truncated: bool | None


@dataclass(frozen=True)
class JobResult:
    running: bool
    frames: tuple[CommandFrame, ...]
    exit_code: int | None
    truncated: bool | None
    error_code: str | None
    error_message: str | None


def _sandbox_info(value: Any) -> SandboxInfo:
    return SandboxInfo(value.sandbox_id, value.state, value.generation)


def _command_frame(value: Any) -> CommandFrame:
    return CommandFrame(
        value.kind,
        value.seq,
        bytes(value.data) if value.data is not None else None,
        value.exit_code,
        value.truncated,
    )


class CommandStream(AsyncIterator[CommandFrame]):
    def __init__(self, handle: Any) -> None:
        self._handle = handle

    def __aiter__(self) -> CommandStream:
        return self

    @translate_errors
    async def __anext__(self) -> CommandFrame:
        value = await self._handle.next()
        if value is None:
            raise StopAsyncIteration
        return _command_frame(value)

    @translate_errors
    async def close(self) -> None:
        await self._handle.close()


class Sandbox:
    def __init__(self, name: str) -> None:
        self._handle = _LazyHandle[Any]("sandbox", name)

    @translate_errors
    async def capabilities(self) -> frozenset[str]:
        return frozenset((await self._handle.get()).capabilities())

    @translate_errors
    async def create(
        self,
        *,
        sandbox_id: str | None = None,
        tenant_key: str | None = None,
        env: Mapping[str, str] | None = None,
        timeout_ms: int | None = None,
    ) -> SandboxInfo:
        value = await (await self._handle.get()).create(
            sandbox_id, tenant_key, dict(env) if env else None, timeout_ms
        )
        return _sandbox_info(value)

    @translate_errors
    async def get(self, sandbox_id: str) -> SandboxInfo | None:
        value = await (await self._handle.get()).get(sandbox_id)
        return _sandbox_info(value) if value is not None else None

    @translate_errors
    async def get_or_create(
        self,
        *,
        sandbox_id: str | None = None,
        tenant_key: str | None = None,
        env: Mapping[str, str] | None = None,
        timeout_ms: int | None = None,
    ) -> tuple[SandboxInfo, bool]:
        value = await (await self._handle.get()).get_or_create(
            sandbox_id, tenant_key, dict(env) if env else None, timeout_ms
        )
        return _sandbox_info(value.sandbox), value.created

    @translate_errors
    async def list(self) -> list[SandboxInfo]:
        return [_sandbox_info(value) for value in await (await self._handle.get()).list()]

    @translate_errors
    async def run_command(
        self,
        sandbox_id: str,
        command: str,
        args: Sequence[str],
        *,
        timeout_ms: int,
        cwd: str | None = None,
        env: Mapping[str, str] | None = None,
    ) -> CommandStream:
        handle = await (await self._handle.get()).run_command(
            sandbox_id, command, list(args), timeout_ms, cwd, dict(env) if env else None
        )
        return CommandStream(handle)

    @translate_errors
    async def start_job(
        self,
        sandbox_id: str,
        command: str,
        args: Sequence[str],
        *,
        timeout_ms: int,
        cwd: str | None = None,
        env: Mapping[str, str] | None = None,
    ) -> str:
        return await (await self._handle.get()).start_job(
            sandbox_id, command, list(args), timeout_ms, cwd, dict(env) if env else None
        )

    @translate_errors
    async def poll_job(
        self, sandbox_id: str, job_id: str, *, since_seq: int | None = None
    ) -> JobResult:
        value = await (await self._handle.get()).poll_job(sandbox_id, job_id, since_seq)
        return JobResult(
            value.running,
            tuple(_command_frame(frame) for frame in value.frames),
            value.exit_code,
            value.truncated,
            value.error_code,
            value.error_message,
        )

    @translate_errors
    async def cancel_job(self, sandbox_id: str, job_id: str) -> None:
        await (await self._handle.get()).cancel_job(sandbox_id, job_id)

    @translate_errors
    async def read_file(self, sandbox_id: str, path: str) -> bytes:
        return bytes(await (await self._handle.get()).read_file(sandbox_id, path))

    @translate_errors
    async def write_file(self, sandbox_id: str, path: str, contents: bytes) -> None:
        await (await self._handle.get()).write_file(sandbox_id, path, list(contents))

    @translate_errors
    async def pause(self, sandbox_id: str) -> None:
        await (await self._handle.get()).pause(sandbox_id)

    @translate_errors
    async def resume(self, sandbox_id: str) -> None:
        await (await self._handle.get()).resume(sandbox_id)

    @translate_errors
    async def terminate(self, sandbox_id: str) -> None:
        await (await self._handle.get()).terminate(sandbox_id)


def storage(name: str) -> Storage:
    return Storage(name)


def ai(name: str) -> Ai:
    return Ai(name)


def key(name: str) -> Key:
    return Key(name)


def kv(name: str) -> Kv:
    return Kv(name)


def queue(name: str) -> Queue:
    return Queue(name)


def vault(name: str) -> Vault:
    return Vault(name)


def postgres(name: str) -> Postgres:
    return Postgres(name)


def container(name: str) -> Container:
    return Container(name)


def worker(name: str) -> Worker:
    return Worker(name)


def sandbox(name: str) -> Sandbox:
    return Sandbox(name)
