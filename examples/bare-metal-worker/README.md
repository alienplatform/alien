# Bare-metal worker

A job worker in Rust that runs on Linux servers you point it at. It takes jobs over HTTP, runs them in-process, and when a new release replaces it, it stops taking jobs, lets the running ones finish, and exits. There is no `alien.ts`: the whole stack is [`alien.toml`](alien.toml).

```toml
name = "acme-jobs"
platforms = ["machines"]

[pool.workers]
cpu = 2
memory = "4Gi"
machines = { default = 1, allowed = [1, 20] }

[daemon.job-worker]
build = { toolchain = "rust", binary-name = "job-worker" }
pool = "workers"
runtime = { network-mode = "host" }
stop-grace-period = "1h"
health-check = { port = 8080, path = "/healthz" }
```

`stop-grace-period = "1h"` is how long a replaced worker may keep running before it is stopped. `runtime = { network-mode = "host" }` makes the worker listen on port 8080 of the server itself, so `curl localhost:8080` on the server reaches it.

## The worker

State is in memory only: jobs are lost if the process dies, and job ids start again at `j-1` after every restart. A real worker would pull jobs from your queue or database instead of taking them over HTTP. The part to copy is the shutdown: [`src/worker.rs`](src/worker.rs) and [`src/main.rs`](src/main.rs).

| Method | Path | Description |
|--------|------|-------------|
| GET | `/healthz` | `200 {"status":"ok","version":"0.1.0"}`. After SIGTERM or SIGINT: `503 {"status":"draining",...}` |
| POST | `/jobs` | Submit a job. `202 {"id":"j-1","status":"queued"}`. `429` when the queue is full, `503` while draining |
| GET | `/jobs/<id>` | `{"id","task","status","started_at","finished_at","version",...}`. `status` is `queued`, `running`, `finished` or `failed`. `version` is the worker version that ran the job |
| GET | `/jobs` | The 50 most recent jobs, newest first |

Two kinds of job:

```bash
curl -X POST localhost:8080/jobs -d '{"task":"wait","seconds":300}'   # runs for 300 seconds
curl -X POST localhost:8080/jobs -d '{"task":"primes","limit":5000000}' # counts primes, keeps a CPU busy
```

The log is plain lines, one per event:

```
job j-1 started (wait 300s)
received SIGINT
draining: 1 job running, new jobs refused
job j-1 finished in 300s
drained, exiting
```

When the worker is told to stop (Alien sends SIGINT on a release; SIGTERM works the same), it refuses new jobs, finishes every job it already accepted, and exits with status 0. Settings, all optional: `PORT` (8080), `WORKER_CONCURRENCY` (4), `QUEUE_CAPACITY` (100).

## Run it on a server

You need [Rust](https://rustup.rs) and [Zig](https://ziglang.org) on the machine that runs `alien release`: Alien builds a static Linux (musl) binary with `cargo zigbuild`. It adds the Rust target and `cargo-zigbuild` itself.

```bash
# macOS
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
brew install zig

# Linux
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
sudo snap install zig --classic --beta     # or download a release from https://ziglang.org/download
```

```bash
alien login
alien init bare-metal-worker && cd bare-metal-worker
alien release
```

Then, on the Linux server, join the `workers` pool with the join token from your deployment:

```bash
sudo alien-deploy join --token <join-token> --pool workers
```

The pool asks for at least 2 CPUs and 4Gi of memory. Lower `cpu` and `memory` in `alien.toml` for a smaller server.

Check that it is up, and start a job that takes five minutes:

```bash
curl localhost:8080/healthz
curl -X POST localhost:8080/jobs -d '{"task":"wait","seconds":300}'
curl localhost:8080/jobs/j-1
```

## Release again while a job runs

Change `version` in `Cargo.toml` to `0.2.0`, then:

```bash
alien release
```

The server's running worker gets SIGINT. Because the worker listens on the host network, the new version cannot start until the old one has exited, so while `j-1` runs:

- `curl localhost:8080/jobs/j-1` still answers, with `"status":"running"` and `"version":"0.1.0"`.
- `curl localhost:8080/healthz` returns 503 and `POST /jobs` is refused.

When `j-1` finishes, the old worker exits, 0.2.0 starts, and new jobs run on 0.2.0. A job that was running is never cut short, as long as it ends within `stop-grace-period`.

## Tests

```bash
cargo test
```

The unit tests run the queue and the HTTP API on a paused clock, including the drain. `tests/signals.rs` starts the real binary, sends it SIGTERM and SIGINT mid-job, and checks that health and submissions return 503, the job finishes, and the process exits cleanly.
