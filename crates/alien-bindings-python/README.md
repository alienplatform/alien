# Alien Python bindings

`alienplatform` gives async Python applications typed access to resources linked by Alien. Provider selection, short-lived credential refresh, error metadata, and cloud behavior stay in the shared Rust binding core.

The package is under development and is not published to a package registry yet.

## Local development

Build an editable local-provider package from the repository root:

```bash
python -m venv .venv
source .venv/bin/activate
python -m pip install 'maturin>=1.9,<2' 'pytest>=8,<10' 'pytest-asyncio>=1,<2'
maturin develop --manifest-path crates/alien-bindings-python/Cargo.toml \
  --no-default-features --features local
pytest -q crates/alien-bindings-python/tests
```

The native extension uses Python's `asyncio` interface and runs Alien futures on its Tokio runtime. Resource factories are lazy: missing or malformed bindings fail when first used with a structured `AlienError`.

## Resources

```python
from alienplatform import ai, container, key, kv, postgres, queue, sandbox, storage, vault, worker
from sqlalchemy.ext.asyncio import create_async_engine

files = storage("files")
await files.put("reports/today.json", b"{}")
assert await files.get("reports/today.json") == b"{}"

cache = kv("cache")
await cache.put("status", b"ready", condition="absent")

jobs = queue("jobs")
await jobs.send({"kind": "index"})

secrets = vault("secrets")
api_token = await secrets.get("api-token")

database = await postgres("database").connection()
engine = create_async_engine(**database.sqlalchemy_async_engine_kwargs())

service_url = await container("api").internal_url()

response = await worker("processor").invoke(
    method="POST", path="/jobs", body=b"{}", timeout_ms=30_000
)

ciphertext = await key("records").encrypt(b"private", {"tenant": "tenant-42"})

model = await ai("assistant").connection()
# OpenAI-compatible clients use model.openai_base_url. model.api_key is None
# for ambient Bedrock, Vertex, and Foundry resources because the embedded Rust
# gateway authenticates with the workload identity.
```

Do not log or render Postgres connection strings, passwords, vault values, or model credentials.
Connection object representations redact credentials by default.

## AI

`ai(name).connection()` returns a provider-neutral base URL. Managed Bedrock,
Vertex, and Foundry resources start the same in-process Rust gateway used by the
TypeScript SDK, so signing and short-lived credential refresh stay in the shared
implementation. External OpenAI and Anthropic bindings return the projected key
and provider endpoint. Pass `openai_base_url` to an OpenAI-compatible client.

Sandbox commands stream raw stdout and stderr frames with production-order sequence numbers:

```python
runtime = sandbox("runtime")
instance, created = await runtime.get_or_create(tenant_key="tenant-42", timeout_ms=300_000)
stream = await runtime.run_command(
    instance.sandbox_id,
    "python",
    ["-c", "print('hello')"],
    timeout_ms=30_000,
)
try:
    async for frame in stream:
        if frame.data is not None:
            print(frame.data.decode(errors="replace"), end="")
        if frame.exit_code is not None and frame.exit_code != 0:
            raise RuntimeError(f"command exited with {frame.exit_code}")
finally:
    await stream.close()
```

Long-running commands can use `start_job`, `poll_job`, and `cancel_job`. Call `capabilities()` before optional file, job, pause/resume, or isolation behavior because support differs by platform.

## PostgreSQL TLS

`Postgres.connection()` returns the provider-neutral connection parameters, a wire-protocol URL, CA certificates, and SSL mode. `sqlalchemy_async_engine_kwargs()` translates the URL and supplies the verified TLS context together so `create_async_engine()` cannot omit provider CA certificates. Lower-level integrations can use `sqlalchemy_async_url()` and `ssl_context()` directly.

## Errors

`AlienError` includes stable `code`, `context`, `retryable`, `internal`, `http_status_code`, and `hint` fields. Branch on `code` rather than parsing the message. Errors from object storage are intentionally mapped without object paths or provider response bodies so application logs do not leak customer data.
