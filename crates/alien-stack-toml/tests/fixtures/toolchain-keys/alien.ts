import * as alien from "@alienplatform/core"

const pythonAgent = new alien.Daemon("python-agent")
  .code({
    type: "source",
    src: "agents/python",
    toolchain: {
      type: "python",
      command: ["python", "-m", "agent"],
      package: "acme-agent",
      pythonVersion: "3.12",
    },
  })
  .build()

const dockerAgent = new alien.Daemon("docker-agent")
  .code({
    type: "source",
    src: "agents",
    toolchain: {
      type: "docker",
      dockerfile: "agents/Dockerfile",
      target: "runtime",
      buildArgs: { VERSION: "2.1", FEATURES: "tls" },
    },
  })
  .build()

const dockerWorker = new alien.Worker("docker-worker")
  .code({
    type: "source",
    src: "workers",
    toolchain: {
      type: "docker",
      dockerfile: "workers/Dockerfile",
      target: "runtime",
      buildArgs: { VERSION: "2.1" },
    },
  })
  .permissions("docker-worker")
  .build()

const pythonWorker = new alien.Worker("python-worker")
  .code({
    type: "source",
    src: "workers/python",
    toolchain: {
      type: "python",
      command: ["python", "worker.py"],
      package: "acme-worker",
      pythonVersion: "3.11",
    },
  })
  .permissions("python-worker")
  .build()

const pythonBox = new alien.Sandbox("python-box")
  .code({
    type: "source",
    src: "boxes/python",
    toolchain: {
      type: "python",
      command: ["python", "-m", "box"],
      package: "acme-box",
      pythonVersion: "3.12",
    },
  })
  .egress("none")
  .lifecycle({})
  .build()

const dockerBox = new alien.Sandbox("docker-box")
  .code({
    type: "source",
    src: "boxes",
    toolchain: {
      type: "docker",
      dockerfile: "boxes/Dockerfile",
      target: "runtime",
      buildArgs: { VERSION: "2.1" },
    },
  })
  .egress("none")
  .lifecycle({})
  .build()

export default new alien.Stack("acme-toolchain-keys")
  .platforms(["machines", "aws"])
  .add(pythonAgent, "live")
  .add(dockerAgent, "live")
  .add(dockerWorker, "live")
  .add(pythonWorker, "live")
  .add(pythonBox, "frozen")
  .add(dockerBox, "frozen")
  .permissions({ profiles: { "docker-worker": {}, "python-worker": {} } })
  .build()
