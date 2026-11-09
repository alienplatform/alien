import * as alien from "@alienplatform/core"

function container(id: string, code: alien.ContainerCode) {
  return new alien.Container(id).code(code).cpu(1).memory("1Gi").permissions(id).build()
}

const prebuilt = container("prebuilt", { type: "image", image: "ghcr.io/acme/prebuilt:2.0.0" })

const rustDefault = container("rust-default", {
  type: "source",
  src: ".",
  toolchain: { type: "rust", binaryName: "rust-default" },
})

const rustNested = container("rust-nested", {
  type: "source",
  src: "services/nested",
  toolchain: { type: "rust", binaryName: "rust-nested" },
})

const typescriptDefault = container("typescript-default", {
  type: "source",
  src: ".",
  toolchain: { type: "typescript" },
})

const typescriptNamed = container("typescript-named", {
  type: "source",
  src: "services/web",
  toolchain: { type: "typescript", binaryName: "server" },
})

const pythonDefault = container("python-default", {
  type: "source",
  src: ".",
  toolchain: { type: "python", command: ["python", "-m", "app"] },
})

const pythonFull = container("python-full", {
  type: "source",
  src: "services/api",
  toolchain: {
    type: "python",
    command: ["gunicorn", "app:server"],
    package: "acme-api",
    pythonVersion: "3.12",
  },
})

const dockerDefault = container("docker-default", {
  type: "source",
  src: ".",
  toolchain: { type: "docker" },
})

const dockerFull = container("docker-full", {
  type: "source",
  src: "services/go",
  toolchain: {
    type: "docker",
    dockerfile: "docker/Dockerfile.prod",
    target: "runtime",
    buildArgs: { GO_VERSION: "1.23", FEATURES: "tls,metrics" },
  },
})

const agent = new alien.Daemon("agent")
  .code({
    type: "source",
    src: ".",
    toolchain: { type: "docker", dockerfile: "agent/Dockerfile" },
  })
  .build()

const processor = new alien.Worker("processor")
  .code({
    type: "source",
    src: ".",
    toolchain: { type: "python", command: ["python", "processor.py"] },
  })
  .permissions("processor")
  .build()

export default new alien.Stack("acme-toolchains")
  .platforms(["aws"])
  .add(prebuilt, "live")
  .add(rustDefault, "live")
  .add(rustNested, "live")
  .add(typescriptDefault, "live")
  .add(typescriptNamed, "live")
  .add(pythonDefault, "live")
  .add(pythonFull, "live")
  .add(dockerDefault, "live")
  .add(dockerFull, "live")
  .add(agent, "live")
  .add(processor, "live")
  .permissions({
    profiles: {
      prebuilt: {},
      "rust-default": {},
      "rust-nested": {},
      "typescript-default": {},
      "typescript-named": {},
      "python-default": {},
      "python-full": {},
      "docker-default": {},
      "docker-full": {},
      processor: {},
    },
  })
  .build()
