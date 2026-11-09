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

When the worker is told to stop (Alien sends SIGINT on a release; SIGTERM works the same), it refuses new jobs, finishes every job it already accepted, and exits with status 0. Settings, all optional: `PORT` (8080), `WORKER_CONCURRENCY` (4, at least 1), `QUEUE_CAPACITY` (100, at least 1). The worker exits at startup if either is 0. A `wait` job runs for at most 3600 seconds, the `stop-grace-period`.

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
alien link
```

In the dashboard, open Settings > Artifacts > CLI and set the setup CLI command name. Then release and create a deployment for your server:

```bash
alien release
alien deploy --name lab --machines
```

`alien deploy --machines` prints a join command like this one:

```bash
curl -fsSL https://pkg.alien.dev/<workspace>/<project>/install.sh | sudo bash -s -- join --token ...
```

Run it on the Linux server. The stack has one pool, so the server joins it without further options. The pool asks for at least 2 CPUs and 4Gi of memory; lower `cpu` and `memory` in `alien.toml` for a smaller server.

On the server, check that it is up, and start a job that takes five minutes:

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

When `j-1` finishes, the old worker exits, 0.2.0 starts a few seconds later, and new jobs run on 0.2.0. A job that is already running is not cut short: no job runs longer than the `stop-grace-period` of 1h. Jobs still waiting in the queue behind it also run before the old worker exits, so with a deep queue the total can pass the grace period and the remainder is stopped.

Because job state is in memory, 0.2.0 does not know about `j-1`: `curl localhost:8080/jobs/j-1` returns 404 once the new worker is up.

## Roll back

Every release has an id. List them and promote the earlier one:

```bash
alien releases ls
alien releases promote <release-id> --channel production
```

The server replaces the running worker with the earlier version the same way: the current one drains, then the earlier one starts. With no job running, `curl localhost:8080/healthz` shows the earlier version again within about 20 seconds.

A release does not have to be healthy to be rolled out. If a new worker's `/healthz` keeps failing, the old worker is already gone (it shares the host network, so it stops first) and the deployment stays in `updating` instead of `running`. Promote the earlier release to get back.

## Tests

```bash
cargo test
```

The unit tests run the queue and the HTTP API on a paused clock, including the drain. `tests/signals.rs` starts the real binary, sends it SIGTERM and SIGINT mid-job, and checks that health and submissions return 503, the job finishes, and the process exits cleanly.
