# Python SDK

The Python SDK uses **decorators** for handlers and a **run function** that manages the event loop.

### Basic Setup

```python
from alien_sdk import run

# ... register handlers ...

if __name__ == "__main__":
    run()
```

### Bindings

```python
from alien_sdk import storage, kv

bucket = storage("my-bucket")
await bucket.put("key", data)

cache = kv("my-cache")
await cache.get("key")
```

### Events

Use decorators:

```python
from alien_sdk import on_storage_event, on_cron_event, on_queue_message

@on_storage_event("uploads")
async def handle_upload(event):
    print(f"File {event.key} was {event.type}")

@on_cron_event("daily-cleanup")
async def cleanup():
    await cleanup_old_files()

@on_queue_message("tasks")
async def handle_task(message):
    await process_task(message.payload)
```

### Commands

```python
from alien_sdk import command

@command("health-check")
async def health_check(params):
    return {"healthy": True}

@command("generate-report")
async def generate_report(params):
    url = await do_generate_report(params["report_type"])
    return {"url": url}
```

### HTTP Server

Pass your ASGI app to `run()`:

```python
from fastapi import FastAPI
from alien_sdk import run

app = FastAPI()

@app.get("/")
def hello():
    return "Hello!"

if __name__ == "__main__":
    run(app)  # Starts on random port, registers with runtime, enters event loop
```

Works with any ASGI framework (FastAPI, Starlette, etc.).

### waitUntil

```python
from alien_sdk import wait_until

@app.post("/process")
async def process(data: dict):
    wait_until(heavy_processing(data))
    return {"accepted": True}
```

### AlienContext (Advanced)

The global functions use a shared `AlienContext` under the hood. For explicit control or remote bindings:

```python
from alien_sdk import AlienContext

# Explicit context
ctx = await AlienContext.from_env()
bucket = ctx.storage("my-bucket")

# Remote bindings (access resources from outside the runtime)
remote_ctx = await AlienContext.for_remote_agent(agent_id, token)
customer_bucket = remote_ctx.storage("customer-data")
await customer_bucket.put("report.json", report_data)
```

### Full Example

```python
from fastapi import FastAPI
from alien_sdk import storage, run, on_storage_event, command, wait_until

app = FastAPI()

@app.get("/")
def hello():
    return "Hello!"

@app.post("/upload")
async def upload(key: str, data: bytes):
    bucket = storage("uploads")
    await bucket.put(key, data)
    return {"success": True}

@on_storage_event("uploads")
async def handle_upload(event):
    bucket = storage("uploads")
    data = await bucket.get(event.key)
    wait_until(process_file(data))

@command("health")
async def health(params):
    return {"healthy": True}

if __name__ == "__main__":
    run(app)
```

### How It Works

```dockerfile
ENTRYPOINT ["alien-runtime", "--"]
CMD ["python", "-m", "alien_sdk.bootstrap", "main.py"]
```

The bootstrap:
1. Imports the user module (triggers decorator registrations)
2. If `run(app)` is called, starts uvicorn on random port, registers with runtime
3. Enters event loop, dispatches events/commands to decorated handlers

---
