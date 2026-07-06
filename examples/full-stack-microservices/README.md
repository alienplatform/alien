# Full-Stack Microservices

A small support desk application for Kubernetes: seven services behind one public gateway, deployed into the customer's cluster as a single Alien release. It shows how a realistic multi-service product -- UI, API, background jobs, databases, file storage -- runs where the customer's data lives.

## What's included

| Service | Description |
|---------|-------------|
| `gateway` | Public nginx entrypoint. `/health` is served locally, `/api/*` routes to `api`, everything else routes to `dashboard` |
| `dashboard` | TanStack Start UI for creating and processing issues |
| `api` | Bun/Hono API that stores issues in Postgres, queues work in Redis, and stores uploaded files in Alien object storage |
| `worker` | Background processor that consumes Redis jobs, updates Postgres, and writes derived artifacts to object storage |
| `scheduler` | Periodic process that calls `http://api:3000/internal/maintenance` using namespace-local Kubernetes DNS |
| `postgres` | `postgres:16-alpine` with `10Gi` persistent storage mounted at `/data` |
| `redis` | `redis:7-alpine`, run as a third-party image container |
| `files` | Alien `Storage` resource used by `api` and `worker` |

Only `gateway` is public. The other services reach each other by namespace-local Kubernetes service names such as `api`, `postgres`, and `redis`.

### Deployment settings

The application requires one plain deployment variable and one secret:

| Name | Type | Used by |
| --- | --- | --- |
| `APP_PUBLIC_NAME` | plain | `api` |
| `APP_SECRET` | secret | `api`, `worker`, `scheduler` |

`APP_SECRET` is intentionally required at process startup. If it is missing, the service exits with a clear error.

## Local development

Clone the repo and install dependencies from the examples workspace:

```bash
git clone https://github.com/alienplatform/alien
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

## Build and release to Kubernetes

```bash
cd full-stack-microservices
alien build --platform kubernetes
alien release --platform kubernetes
```

The rendered release contains five first-party Dockerfile-based containers, two third-party image containers, one persistent Postgres volume, and one Alien storage resource. The public endpoint is the `gateway` URL:

- `GET /health`
- `GET /api/health`
- `POST /api/issues`
- `GET /api/issues`
- `POST /api/issues/:id/process`

## Learn more

- [How Alien Works](https://alien.dev/docs/how-alien-works)
- [Storage reference](https://alien.dev/docs/infrastructure/storage)
- [alien.dev](https://alien.dev) -- ship to your customer's cloud, keep it fully managed
