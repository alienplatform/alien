// EXAMPLE: Zero-disk vector database deployed to the customer's cloud

import * as alien from "@alienplatform/core"

// Object storage for vector data
// S3 on AWS, Cloud Storage on GCP, Blob Storage on Azure
const storage = new alien.Storage("data").build()

// Writer container
const writer = new alien.Container("writer")
  .code({
    type: "source",
    toolchain: { type: "rust", binaryName: "byocdb" },
    src: ".",
  })
  .cpu(0.5)
  .memory("512Mi")
  .port(8081)
  .environment({
    BYOCDB_MODE: "writer",
    PORT: "8081",
    RUST_LOG: "info,byocdb=debug",
  })
  .permissions("default")
  .link(storage)
  .build()

// Reader container
const reader = new alien.Container("reader")
  .code({
    type: "source",
    toolchain: {
      type: "rust",
      binaryName: "byocdb",
    },
    src: ".",
  })
  .cpu(0.5)
  .memory("512Mi")
  .port(8082)
  .environment({
    BYOCDB_MODE: "reader",
    PORT: "8082",
    RUST_LOG: "info,byocdb=debug",
  })
  .permissions("default")
  .link(storage)
  .build()

// Router: nginx routing requests to writer/reader
const router = new alien.Container("router")
  .code({
    type: "source",
    toolchain: {
      type: "docker",
    },
    src: "./router",
  })
  .cpu(0.25)
  .memory("256Mi")
  .port(8080)
  .expose("http")
  .permissions("default")
  .build()

export default new alien.Stack("byoc-database")
  .add(storage, "frozen")
  .add(writer, "live")
  .add(reader, "live")
  .add(router, "live")
  .permissions({
    profiles: {
      default: {
        data: ["storage/data-read", "storage/data-write"],
      },
    },
  })
  .build()
