# Full-Stack Microservices

This example is a small support desk application for Kubernetes deployments.
It exercises a more realistic shape than a single worker:

- `gateway`: public nginx entrypoint. `/health` is served locally, `/api/*` is routed to `api`, and everything else is routed to `dashboard`.
- `dashboard`: TanStack Start UI for creating and processing issues.
- `api`: Bun/Hono API that stores issues in Postgres, queues work in Redis, and stores uploaded files in Alien object storage.
- `worker`: background processor that consumes Redis jobs, updates Postgres, and writes derived artifacts to object storage.
- `scheduler`: periodic process that calls `http://api:3000/internal/maintenance` using Kubernetes namespace-local DNS.
- `postgres`: `postgres:16-alpine` with `10Gi` persistent storage mounted at `/data`.
- `redis`: `redis:7-alpine`, modeled as a normal container for now. TODO: replace this with a first-class `alien.Redis` resource when that exists.
- `files`: Alien `Storage` resource used by `api` and `worker`.

Only `gateway` is public. The other services are reachable by namespace-local
Kubernetes service names such as `api`, `postgres`, and `redis`.

## Deployment Settings

The application requires one plain deployment variable and one secret deployment
variable in the app path:

| Name | Type | Used by |
| --- | --- | --- |
| `APP_PUBLIC_NAME` | plain | `api` |
| `APP_SECRET` | secret | `api`, `worker`, `scheduler` |

`APP_SECRET` is intentionally required at process startup. If it is missing, the
service exits with a clear error.

## Local Development

Install dependencies from the examples workspace:

```bash
cd alien/examples
pnpm install
```

Run TypeScript checks:

```bash
pnpm -C full-stack-microservices test:ts
```

For manual local service testing, run Postgres and Redis locally and set:

```bash
DATABASE_URL=postgres://app:app@localhost:5432/app?sslmode=disable
REDIS_URL=redis://localhost:6379
APP_SECRET=dev-secret
```

The API listens on `PORT` and defaults to `3000`.

## Kubernetes Build and Release

The stack supports Kubernetes:

```bash
cd alien/examples/full-stack-microservices
alien build --platform kubernetes
alien release --platform kubernetes
```

The rendered stack should contain five first-party Dockerfile-based containers,
two third-party image containers, one persistent Postgres volume, and one Alien
storage resource. The public endpoint is the `gateway` URL:

- `GET /health`
- `GET /api/health`
- `POST /api/issues`
- `GET /api/issues`
- `POST /api/issues/:id/process`

Project `5-e2e-app-selector` will register this example as the
`full-stack-microservices` E2E app and add app-specific assertions for gateway
routing, Postgres persistence, Redis-backed processing, object storage writes,
and namespace-local service DNS.
