import * as alien from "@alienplatform/core"

const files = new alien.Storage("files").build()

const postgres = new alien.Container("postgres")
  .code({ type: "image", image: "postgres:16-alpine" })
  .cpu(0.5)
  .memory("512Mi")
  .port(5432)
  .environment({
    POSTGRES_DB: "app",
    POSTGRES_USER: "app",
    POSTGRES_PASSWORD: "app",
    PGDATA: "/data/postgres",
  })
  .permissions("app")
  .persistentStorage("10Gi")
  .build()

const redis = new alien.Container("redis")
  .code({ type: "image", image: "redis:7-alpine" })
  .cpu(0.25)
  .memory("256Mi")
  .port(6379)
  .permissions("app")
  .build()

const api = new alien.Container("api")
  .code({
    type: "source",
    src: "./services/api",
    toolchain: { type: "typescript" },
  })
  .cpu(0.5)
  .memory("512Mi")
  .port(3000)
  .environment({
    PORT: "3000",
    DATABASE_URL: "postgres://app:app@postgres:5432/app?sslmode=disable",
    REDIS_URL: "redis://redis:6379",
    FILES_BUCKET: "files",
    APP_PUBLIC_NAME: "Alien Support Desk",
  })
  .permissions("app")
  .link(files)
  .build()

const worker = new alien.Container("worker")
  .code({
    type: "source",
    src: "./services/worker",
    toolchain: { type: "typescript" },
  })
  .cpu(0.25)
  .memory("512Mi")
  .environment({
    DATABASE_URL: "postgres://app:app@postgres:5432/app?sslmode=disable",
    REDIS_URL: "redis://redis:6379",
    FILES_BUCKET: "files",
  })
  .permissions("app")
  .link(files)
  .build()

const scheduler = new alien.Container("scheduler")
  .code({
    type: "source",
    src: "./services/scheduler",
    toolchain: { type: "docker", dockerfile: "Dockerfile" },
  })
  .cpu(0.25)
  .memory("256Mi")
  .environment({
    API_URL: "http://api:3000",
    SCHEDULE_INTERVAL_SECONDS: "60",
  })
  .permissions("app")
  .build()

const dashboard = new alien.Container("dashboard")
  .code({
    type: "source",
    src: "./services/dashboard",
    toolchain: { type: "docker", dockerfile: "Dockerfile" },
  })
  .cpu(0.25)
  .memory("256Mi")
  .port(3000)
  .environment({
    PORT: "3000",
    VITE_API_BASE: "/api",
  })
  .permissions("app")
  .build()

const gateway = new alien.Container("gateway")
  .code({
    type: "source",
    src: "./services/gateway",
    toolchain: { type: "docker", dockerfile: "Dockerfile" },
  })
  .cpu(0.25)
  .memory("128Mi")
  .port(8080)
  .expose("http")
  .healthCheck({ path: "/health", method: "GET", timeoutSeconds: 1, failureThreshold: 3 })
  .permissions("app")
  .build()

export default new alien.Stack("full-stack-microservices")
  .platforms(["kubernetes"])
  .add(files, "frozen")
  .add(postgres, "live")
  .add(redis, "live")
  .add(api, "live")
  .add(worker, "live")
  .add(scheduler, "live")
  .add(dashboard, "live")
  .add(gateway, "live")
  .permissions({
    profiles: {
      app: {
        files: ["storage/data-read", "storage/data-write"],
      },
    },
  })
  .build()
