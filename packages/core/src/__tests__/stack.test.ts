import { describe, expect, it } from "vitest"

import { WorkerSchema } from "../generated/index.js"
import * as alien from "../index.js"

// Shared image URI for workers in tests
const SHARED_IMAGE = "docker.io/library/rust:latest"

describe("Stack builder validation", () => {
  it("builds compute pools from portable requirements only", () => {
    const compute = new alien.ComputeCluster("runtime")
      .pool("nested", {
        requirements: {
          cpu: 4,
          memory: "16Gi",
          architecture: "x86_64",
          nestedVirtualization: true,
        },
        scale: {
          type: "fixed",
          machines: { min: 2, max: 4, default: 2 },
        },
      })
      .build()

    expect(compute.config.capacityGroups).toEqual([
      {
        groupId: "nested",
        profile: {
          cpu: "4",
          memoryBytes: 17179869184,
          ephemeralStorageBytes: 21474836480,
          architecture: "x86_64",
          gpu: undefined,
        },
        minSize: 2,
        maxSize: 2,
        scalePolicy: {
          type: "fixed",
          machines: { min: 2, max: 4, default: 2 },
        },
        nestedVirtualization: true,
      },
    ])
  })

  it("builds stack input definitions for deployment forms", () => {
    const stackInputs = alien.inputs({
      apiBaseUrl: alien.string({
        providedBy: ["developer", "deployer"],
        required: true,
        label: "API base URL",
        description: "Public URL used by the runtime service.",
        placeholder: "https://api.example.com",
        format: "url",
        env: "API_BASE_URL",
      }),
      accessKey: alien.secret({
        providedBy: "deployer",
        required: true,
        label: "Access key",
        description: "Secret token used by the runtime service.",
        minLength: 1,
        env: {
          name: "ACCESS_KEY",
          targetResources: ["my-test-worker"],
          type: "secret",
        },
      }),
      deploymentTier: alien.enum(["starter", "enterprise"], {
        providedBy: "developer",
        required: false,
        label: "Deployment tier",
        description: "Controls default service sizing.",
        default: "starter",
      }),
    })

    const worker = new alien.Worker("my-test-worker")
      .code({ type: "image", image: SHARED_IMAGE })
      .permissions("execution")
      .build()

    const stack = new alien.Stack("my-test-stack").inputs(stackInputs).add(worker, "live").build()
    const inputs = stack.inputs
    expect(inputs).toBeDefined()
    if (!inputs) {
      throw new Error("expected stack inputs to be defined")
    }

    expect(inputs).toHaveLength(3)
    expect(inputs.map(input => input.id)).toEqual(["apiBaseUrl", "accessKey", "deploymentTier"])
    expect(inputs.find(input => input.id === "apiBaseUrl")).toMatchObject({
      kind: "string",
      providedBy: ["developer", "deployer"],
      required: true,
      validation: { format: "url" },
      env: [{ name: "API_BASE_URL" }],
    })
    expect(inputs.find(input => input.id === "accessKey")).toMatchObject({
      kind: "secret",
      providedBy: ["deployer"],
      env: [
        {
          name: "ACCESS_KEY",
          targetResources: ["my-test-worker"],
          type: "secret",
        },
      ],
    })
    expect(inputs.find(input => input.id === "deploymentTier")).toMatchObject({
      kind: "enum",
      default: {
        type: "string",
        value: "starter",
      },
      validation: {
        values: ["starter", "enterprise"],
      },
    })
  })

  it("rejects non-portable stack input regex patterns", () => {
    expect(() =>
      alien.inputs({
        apiBaseUrl: alien.string({
          providedBy: "deployer",
          required: true,
          label: "API base URL",
          description: "Public URL used by the runtime service.",
          pattern: "(?=https://).*",
        }),
      }),
    ).toThrow(/not portable/)
  })

  it("builds a stateful container with persistent storage options", () => {
    const postgres = new alien.Container("postgres")
      .code({ type: "image", image: "postgres:16-alpine" })
      .cpu(0.5)
      .memory("512Mi")
      .port(5432)
      .permissions("database")
      .persistentStorage("20Gi", {
        mountPath: "/var/lib/postgresql/data",
      })
      .build()

    expect(postgres.config.stateful).toBe(true)
    expect(postgres.config.persistentStorage).toEqual({
      size: "20Gi",
      mountPath: "/var/lib/postgresql/data",
    })
  })

  it("builds container and daemon stop grace periods", () => {
    const container = new alien.Container("api")
      .code({ type: "image", image: "nginx:latest" })
      .cpu(0.25)
      .memory("256Mi")
      .permissions("execution")
      .stopGracePeriod(21_600)
      .build()

    expect(container.config.stopGracePeriodSeconds).toBe(21_600)

    const daemon = new alien.Daemon("log-forwarder")
      .code({ type: "image", image: "registry.example.com/log-forwarder:latest" })
      .permissions("execution")
      .stopGracePeriod(21_600)
      .build()

    expect(daemon.config.stopGracePeriodSeconds).toBe(21_600)
  })

  it("builds container and daemon wildcard public endpoint options", () => {
    const container = new alien.Container("router")
      .code({ type: "image", image: "nginx:latest" })
      .cpu(0.25)
      .memory("256Mi")
      .permissions("execution")
      .publicEndpoint("api", 8080, {
        protocol: "http",
        hostLabel: "edge",
        wildcardSubdomains: true,
      })
      .build()

    expect(container.config.ports).toEqual([
      {
        port: 8080,
      },
    ])
    expect(container.config.publicEndpoints).toEqual([
      {
        name: "api",
        port: 8080,
        protocol: "http",
        hostLabel: "edge",
        wildcardSubdomains: true,
      },
    ])

    const daemon = new alien.Daemon("gateway")
      .code({ type: "image", image: "registry.example.com/gateway:latest" })
      .cluster("compute")
      .permissions("execution")
      .publicEndpoint("api", 8080, {
        protocol: "http",
        hostLabel: "public",
        wildcardSubdomains: true,
      })
      .healthCheck({
        path: "/health",
        method: "GET",
        timeoutSeconds: 1,
        failureThreshold: 3,
      })
      .build()

    expect(daemon.config.publicEndpoints).toEqual([
      {
        name: "api",
        port: 8080,
        protocol: "http",
        hostLabel: "public",
        wildcardSubdomains: true,
      },
    ])
    expect(daemon.config.healthCheck).toEqual({
      path: "/health",
      method: "GET",
      timeoutSeconds: 1,
      failureThreshold: 3,
    })
  })

  it("defaults container commandsEnabled to false and allows enabling it", () => {
    const defaultContainer = new alien.Container("api")
      .code({ type: "image", image: "api:latest" })
      .cpu(0.5)
      .memory("512Mi")
      .port(8080)
      .permissions("execution")
      .build()

    expect(defaultContainer.config.commandsEnabled).toBe(false)

    const commandsContainer = new alien.Container("cmd-api")
      .code({ type: "image", image: "api:latest" })
      .cpu(0.5)
      .memory("512Mi")
      .port(8080)
      .permissions("execution")
      .commandsEnabled(true)
      .build()

    expect(commandsContainer.config.commandsEnabled).toBe(true)
  })

  it("builds and validates a complex stack with permissions", () => {
    // Storage bucket
    const storage = new alien.Storage("my-test-bucket").publicRead(true).build()

    // Main application worker with permissions
    const worker = new alien.Worker("my-test-worker")
      .code({ type: "image", image: SHARED_IMAGE })
      .memoryMb(512)
      .timeoutSeconds(30)
      .permissions("execution")
      .publicEndpoint("api")
      .environment({
        RUST_LOG: "info,alien_runtime_test_server=debug,alien_runtime=debug",
      })
      .link(storage)
      .build()

    const stack = new alien.Stack("my-test-stack")
      .add(storage, "frozen")
      .add(worker, "live")
      .permissions({
        profiles: {
          execution: {
            "*": ["storage/data-read"],
            "my-test-bucket": ["storage/data-write"],
          },
        },
        management: {
          extend: {
            "*": ["worker/management", "storage/management"],
          },
        },
      })
      .build()

    // Basic assertions
    expect(stack.id).toBe("my-test-stack")
    expect(stack.resources).toHaveProperty("my-test-bucket")
    expect(stack.resources).toHaveProperty("my-test-worker")
    expect(stack.permissions?.profiles).toHaveProperty("execution")
    expect(stack.permissions?.management).toHaveProperty("extend")

    // Schema validation occurs inside build(); absence of thrown error means success

    // Snapshot the full stack for regression testing
    expect(stack).toMatchSnapshot()
  })

  it.each([0, 3601])("rejects unsupported Worker timeout %s", timeoutSeconds => {
    expect(() =>
      new alien.Worker("slow-worker")
        .code({ type: "image", image: SHARED_IMAGE })
        .permissions("execution")
        .timeoutSeconds(timeoutSeconds)
        .build(),
    ).toThrow()
  })

  it("builds and validates a stack with Build and ArtifactRegistry resources", () => {
    // Artifact registry for storing build artifacts
    const registry = new alien.ArtifactRegistry("my-artifact-registry").build()

    // Storage for build inputs/outputs
    const buildStorage = new alien.Storage("build-storage").build()

    // Build resource with permissions
    const build = new alien.Build("my-build")
      .computeType("medium")
      .environment({
        NODE_ENV: "production",
        BUILD_TARGET: "release",
      })
      .link(registry)
      .link(buildStorage)
      .permissions("builder")
      .build()

    const stack = new alien.Stack("build-stack")
      .add(registry, "frozen")
      .add(buildStorage, "frozen")
      .add(build, "live")
      .permissions({
        profiles: {
          builder: {
            "*": ["artifact-registry/data-read", "artifact-registry/data-write"],
            "build-storage": ["storage/data-read", "storage/data-write"],
          },
        },
        management: {
          extend: {
            "*": ["build/management", "storage/management", "artifact-registry/management"],
          },
        },
      })
      .build()

    // Basic assertions
    expect(stack.id).toBe("build-stack")
    expect(stack.resources).toHaveProperty("my-artifact-registry")
    expect(stack.resources).toHaveProperty("build-storage")
    expect(stack.resources).toHaveProperty("my-build")

    // Verify resource configurations
    const buildResource = stack.resources["my-build"]
    expect(buildResource?.config.computeType).toBe("medium")
    expect(buildResource?.config.environment).toEqual({
      NODE_ENV: "production",
      BUILD_TARGET: "release",
    })
    expect(buildResource?.config.links).toHaveLength(2)

    // Schema validation occurs inside build(); absence of thrown error means success
    expect(stack).toMatchSnapshot()
  })

  it("builds and validates a stack with worker source", () => {
    const workerWithSource = new alien.Worker("my-source-worker")
      .code({
        type: "source",
        src: "./app",
        toolchain: { type: "typescript" },
      })
      .memoryMb(256)
      .timeoutSeconds(15)
      .permissions("execution")
      .build()

    const stack = new alien.Stack("my-source-stack")
      .add(workerWithSource, "live")
      .permissions({
        profiles: {
          execution: {
            "*": ["worker/execute"],
          },
        },
        management: {
          extend: {
            "*": ["worker/management"],
          },
        },
      })
      .build()

    expect(stack.id).toBe("my-source-stack")
    expect(stack.resources).toHaveProperty("my-source-worker")
    const resourceInStack = stack.resources["my-source-worker"]
    expect(resourceInStack).toBeDefined()
    const workerConfigFromStack = WorkerSchema.parse(resourceInStack!.config)
    expect(workerConfigFromStack.code.type).toBe("source")

    expect(stack).toMatchSnapshot()
  })
})

describe("Permissions system", () => {
  it("creates a stack with custom permission sets", () => {
    // Create a custom permission set
    const customPermissionSet: alien.PermissionSet = {
      id: "custom-storage-access",
      description: "Custom storage access permissions",
      platforms: {
        aws: [
          {
            grant: { actions: ["s3:GetObject", "s3:PutObject"] },
            binding: {
              stack: { resources: ["arn:aws:s3:::${stackPrefix}-*"] },
            },
          },
        ],
      },
    }

    // Create a worker with permissions
    const worker = new alien.Worker("test-worker")
      .code({ type: "image", image: SHARED_IMAGE })
      .permissions("execution")
      .build()

    // Create stack with both string and custom permission sets
    const stack = new alien.Stack("permissions-stack")
      .add(worker, "live")
      .permissions({
        profiles: {
          execution: {
            "*": ["storage/data-read", customPermissionSet],
          },
        },
        management: {
          extend: {
            "*": ["worker/management"],
          },
        },
      })
      .build()

    // Verify the stack is properly configured
    expect(stack.id).toBe("permissions-stack")
    expect(stack.resources).toHaveProperty("test-worker")
    expect(stack.permissions?.profiles).toHaveProperty("execution")
    expect(stack.permissions?.management).toHaveProperty("extend")

    // Verify the permissions structure
    expect(stack.permissions?.profiles.execution?.["*"]).toEqual([
      "storage/data-read",
      customPermissionSet,
    ])

    expect(stack).toMatchSnapshot()
  })

  it("splits a gated reference into a plain entry plus a permission gate", () => {
    const io = alien.inputs({
      dataWrite: alien.boolean({
        providedBy: "deployer",
        required: false,
        label: "Data write access",
        description: "Whether the workload may write data",
        env: "APP_DATA_WRITE",
      }),
    })
    const worker = new alien.Worker("proxy-worker")
      .code({ type: "image", image: SHARED_IMAGE })
      .permissions("proxy")
      .build()

    const stack = new alien.Stack("gated-stack")
      .add(worker, "live")
      .inputs(io)
      .permissions({
        profiles: {
          proxy: {
            "*": [alien.permission("queue/data-write").enabled(io.dataWrite), "kv/data-write"],
          },
        },
      })
      .build()

    expect(stack.permissions?.profiles.proxy?.["*"]).toEqual(["queue/data-write", "kv/data-write"])
    expect(stack.permissions?.gates).toEqual([
      {
        profile: "proxy",
        resource: "*",
        permissionSetId: "queue/data-write",
        inputId: "dataWrite",
        enabledValue: "true",
      },
    ])
  })

  it("rejects a gate whose input is not declared on the stack", () => {
    const io = alien.inputs({
      dataWrite: alien.boolean({
        providedBy: "deployer",
        required: false,
        label: "Data write access",
        description: "Whether the workload may write data",
        env: "APP_DATA_WRITE",
      }),
    })

    const stack = new alien.Stack("gated-stack").permissions({
      profiles: {
        proxy: { "*": [alien.permission("queue/data-write").enabled(io.dataWrite)] },
      },
    })

    expect(() => stack.build()).toThrow(/not declared on the stack/)
  })

  it("rejects a gate whose input has no env mapping", () => {
    const io = alien.inputs({
      dataWrite: alien.boolean({
        providedBy: "deployer",
        required: false,
        label: "Data write access",
        description: "Whether the workload may write data",
      }),
    })

    const stack = new alien.Stack("gated-stack").inputs(io).permissions({
      profiles: {
        proxy: { "*": [alien.permission("queue/data-write").enabled(io.dataWrite)] },
      },
    })

    expect(() => stack.build()).toThrow(/env mapping/)
  })

  it("rejects gating on a secret input", () => {
    const io = alien.inputs({
      apiKey: alien.secret({
        providedBy: "deployer",
        required: false,
        label: "API key",
        description: "Upstream API key",
        env: "APP_API_KEY",
      }),
    })
    const stack = new alien.Stack("gated-stack").inputs(io).permissions({
      profiles: { proxy: { "*": [alien.permission("queue/data-write").enabled(io.apiKey)] } },
    })

    expect(() => stack.build()).toThrow(/cannot gate on input/)
  })

  it("rejects a set gated differently under a resource key and the wildcard", () => {
    const io = alien.inputs({
      storeEnabled: alien.boolean({
        providedBy: "deployer",
        required: false,
        label: "Store enabled",
        description: "Whether the store grant is included",
        env: "APP_STORE_ENABLED",
      }),
      globalEnabled: alien.boolean({
        providedBy: "deployer",
        required: false,
        label: "Global enabled",
        description: "Whether the global grant is included",
        env: "APP_GLOBAL_ENABLED",
      }),
    })
    // Same set gated on different inputs under a resource key vs. "*": the setup
    // emitter merges the two and would emit the grant unconditionally.
    const stack = new alien.Stack("gated-stack").inputs(io).permissions({
      profiles: {
        execution: {
          store: [alien.permission("kv/data-write").enabled(io.storeEnabled)],
          "*": [alien.permission("kv/data-write").enabled(io.globalEnabled)],
        },
      },
    })

    expect(() => stack.build()).toThrow(/granted under both/)
  })

  it("accepts a set gated identically under a resource key and the wildcard", () => {
    const io = alien.inputs({
      storeEnabled: alien.boolean({
        providedBy: "deployer",
        required: false,
        label: "Store enabled",
        description: "Whether the store grant is included",
        env: "APP_STORE_ENABLED",
      }),
    })
    const worker = new alien.Worker("proxy-worker")
      .code({ type: "image", image: SHARED_IMAGE })
      .permissions("execution")
      .build()
    const stack = new alien.Stack("gated-stack")
      .add(worker, "live")
      .inputs(io)
      .permissions({
        profiles: {
          execution: {
            store: [alien.permission("kv/data-write").enabled(io.storeEnabled)],
            "*": [alien.permission("kv/data-write").enabled(io.storeEnabled)],
          },
        },
      })

    expect(() => stack.build()).not.toThrow()
  })

  it("rejects a resource gate that collides with a hand-written gate on the same grant", () => {
    const io = alien.inputs({
      storeEnabled: alien.boolean({
        providedBy: "deployer",
        required: false,
        label: "Store enabled",
        description: "Whether the store grant is included",
        env: "APP_STORE_ENABLED",
      }),
      otherFlag: alien.boolean({
        providedBy: "deployer",
        required: false,
        label: "Other flag",
        description: "A different gating input",
        env: "APP_OTHER_FLAG",
      }),
    })
    // The resource's own .enabled(storeEnabled) and a hand-written gate on a
    // different input both target execution/store/kv-data-write.
    const store = new alien.Kv("store").enabled(io.storeEnabled).build()
    const stack = new alien.Stack("gated-stack")
      .add(store, "live")
      .inputs(io)
      .permissions({
        profiles: {
          execution: {
            store: [alien.permission("kv/data-write").enabled(io.otherFlag)],
          },
        },
      })

    expect(() => stack.build()).toThrow(/gated on only one input/)
  })

  it("rejects two hand-written gates on the same grant with different inputs", () => {
    const io = alien.inputs({
      flagA: alien.boolean({
        providedBy: "deployer",
        required: false,
        label: "Flag A",
        description: "First gating input",
        env: "APP_FLAG_A",
      }),
      flagB: alien.boolean({
        providedBy: "deployer",
        required: false,
        label: "Flag B",
        description: "Second gating input",
        env: "APP_FLAG_B",
      }),
    })
    const stack = new alien.Stack("gated-stack").inputs(io).permissions({
      profiles: {
        execution: {
          store: [
            alien.permission("kv/data-write").enabled(io.flagA),
            alien.permission("kv/data-write").enabled(io.flagB),
          ],
        },
      },
    })

    expect(() => stack.build()).toThrow(/two conflicting gates/)
  })

  it("rejects a boolean gate with a non-boolean enabled value", () => {
    const io = alien.inputs({
      dataWrite: alien.boolean({
        providedBy: "deployer",
        required: false,
        label: "Data write access",
        description: "Whether the workload may write data",
        env: "APP_DATA_WRITE",
      }),
    })
    const stack = new alien.Stack("gated-stack").inputs(io).permissions({
      profiles: {
        proxy: { "*": [alien.permission("queue/data-write").enabled(io.dataWrite, "on")] },
      },
    })

    expect(() => stack.build()).toThrow(/pass true or false/)
  })

  it("rejects an integer gate with a fractional enabled value", () => {
    const io = alien.inputs({
      replicas: alien.integer({
        providedBy: "deployer",
        required: false,
        label: "Replicas",
        description: "Replica count",
        env: "APP_REPLICAS",
      }),
    })
    const stack = new alien.Stack("gated-stack").inputs(io).permissions({
      profiles: {
        proxy: { "*": [alien.permission("queue/data-write").enabled(io.replicas, "3.5")] },
      },
    })

    expect(() => stack.build()).toThrow(/is not an integer/)
  })

  it("rejects an enum gate whose value is outside the declared domain", () => {
    const io = alien.inputs({
      mode: alien.enum(["on", "off"], {
        providedBy: "deployer",
        required: false,
        label: "Mode",
        description: "Feature mode",
        env: "APP_MODE",
      }),
    })
    const stack = new alien.Stack("gated-stack").inputs(io).permissions({
      profiles: {
        proxy: { "*": [alien.permission("queue/data-write").enabled(io.mode, "nope")] },
      },
    })

    expect(() => stack.build()).toThrow(/not one of/)
  })

  it("rejects gating on a developer-only input", () => {
    const io = alien.inputs({
      tier: alien.enum(["a", "b"], {
        providedBy: "developer",
        required: false,
        label: "Tier",
        description: "Deployment tier",
        env: "APP_TIER",
      }),
    })
    const stack = new alien.Stack("gated-stack").inputs(io).permissions({
      profiles: { proxy: { "*": [alien.permission("queue/data-write").enabled(io.tier, "a")] } },
    })

    expect(() => stack.build()).toThrow(/deployer cannot set/)
  })

  it("lowers a resource's .enabled() into gates on its granted sets", () => {
    const io = alien.inputs({
      kvEnabled: alien.boolean({
        providedBy: "deployer",
        required: false,
        label: "KV enabled",
        description: "Whether the KV store is in use",
        env: "APP_KV_ENABLED",
      }),
    })
    const store = new alien.Kv("store").enabled(io.kvEnabled).build()
    const worker = new alien.Worker("proxy-worker")
      .code({ type: "image", image: SHARED_IMAGE })
      .permissions("app")
      .build()
    const stack = new alien.Stack("gated-stack")
      .add(store, "live")
      .add(worker, "live")
      .inputs(io)
      .permissions({ profiles: { app: { store: ["kv/data-write"] } } })
      .build()

    // No alien.permission() wiring: gating the resource synthesizes the gate.
    expect(stack.permissions?.gates).toEqual([
      {
        profile: "app",
        resource: "store",
        permissionSetId: "kv/data-write",
        inputId: "kvEnabled",
        enabledValue: "true",
      },
    ])
  })

  it("throws when a gated resource has no granted set to gate", () => {
    const io = alien.inputs({
      kvEnabled: alien.boolean({
        providedBy: "deployer",
        required: false,
        label: "KV enabled",
        description: "Whether the KV store is in use",
        env: "APP_KV_ENABLED",
      }),
    })
    const store = new alien.Kv("store").enabled(io.kvEnabled).build()
    const worker = new alien.Worker("proxy-worker")
      .code({ type: "image", image: SHARED_IMAGE })
      .permissions("app")
      .build()
    const stack = new alien.Stack("gated-stack")
      .add(store, "live")
      .add(worker, "live")
      .inputs(io)
      .permissions({ profiles: { app: { "*": ["queue/data-write"] } } })

    expect(() => stack.build()).toThrow(/no profile grants a "kv\/" permission set/)
  })

  it("gates a wildcard-granted set for the only resource of its type", () => {
    const io = alien.inputs({
      kvEnabled: alien.boolean({
        providedBy: "deployer",
        required: false,
        label: "KV enabled",
        description: "Whether the KV store is in use",
        env: "APP_KV_ENABLED",
      }),
    })
    const store = new alien.Kv("store").enabled(io.kvEnabled).build()
    const worker = new alien.Worker("proxy-worker")
      .code({ type: "image", image: SHARED_IMAGE })
      .permissions("app")
      .build()
    const stack = new alien.Stack("gated-stack")
      .add(store, "live")
      .add(worker, "live")
      .inputs(io)
      .permissions({ profiles: { app: { "*": ["kv/data-write"] } } })
      .build()

    expect(stack.permissions?.gates).toEqual([
      {
        profile: "app",
        resource: "*",
        permissionSetId: "kv/data-write",
        inputId: "kvEnabled",
        enabledValue: "true",
      },
    ])
  })

  it("gates a shared wildcard set once when same-type resources agree on the input", () => {
    const io = alien.inputs({
      kvEnabled: alien.boolean({
        providedBy: "deployer",
        required: false,
        label: "KV enabled",
        description: "Whether the KV stores are in use",
        env: "APP_KV_ENABLED",
      }),
    })
    const a = new alien.Kv("a").enabled(io.kvEnabled).build()
    const b = new alien.Kv("b").enabled(io.kvEnabled).build()
    const worker = new alien.Worker("proxy-worker")
      .code({ type: "image", image: SHARED_IMAGE })
      .permissions("app")
      .build()
    const stack = new alien.Stack("gated-stack")
      .add(a, "live")
      .add(b, "live")
      .add(worker, "live")
      .inputs(io)
      .permissions({ profiles: { app: { "*": ["kv/data-write"] } } })
      .build()

    expect(stack.permissions?.gates).toEqual([
      {
        profile: "app",
        resource: "*",
        permissionSetId: "kv/data-write",
        inputId: "kvEnabled",
        enabledValue: "true",
      },
    ])
  })

  it("throws when a wildcard-granted set is shared by an ungated same-type resource", () => {
    const io = alien.inputs({
      kvEnabled: alien.boolean({
        providedBy: "deployer",
        required: false,
        label: "KV enabled",
        description: "Whether the primary store is in use",
        env: "APP_KV_ENABLED",
      }),
    })
    const primary = new alien.Kv("primary").enabled(io.kvEnabled).build()
    const secondary = new alien.Kv("secondary").build()
    const worker = new alien.Worker("proxy-worker")
      .code({ type: "image", image: SHARED_IMAGE })
      .permissions("app")
      .build()
    const stack = new alien.Stack("gated-stack")
      .add(primary, "live")
      .add(secondary, "live")
      .add(worker, "live")
      .inputs(io)
      .permissions({ profiles: { app: { "*": ["kv/data-write"] } } })

    expect(() => stack.build()).toThrow(/wildcard/)
  })

  it("throws when same-type resources gate a shared wildcard set on different inputs", () => {
    const io = alien.inputs({
      flagA: alien.boolean({
        providedBy: "deployer",
        required: false,
        label: "A",
        description: "Store A toggle",
        env: "APP_A",
      }),
      flagB: alien.boolean({
        providedBy: "deployer",
        required: false,
        label: "B",
        description: "Store B toggle",
        env: "APP_B",
      }),
    })
    const a = new alien.Kv("a").enabled(io.flagA).build()
    const b = new alien.Kv("b").enabled(io.flagB).build()
    const worker = new alien.Worker("proxy-worker")
      .code({ type: "image", image: SHARED_IMAGE })
      .permissions("app")
      .build()
    const stack = new alien.Stack("gated-stack")
      .add(a, "live")
      .add(b, "live")
      .add(worker, "live")
      .inputs(io)
      .permissions({ profiles: { app: { "*": ["kv/data-write"] } } })

    expect(() => stack.build()).toThrow(/wildcard/)
  })

  it("builds a valid enum gate via the permission() primitive", () => {
    const io = alien.inputs({
      mode: alien.enum(["on", "off"], {
        providedBy: "deployer",
        required: false,
        label: "Mode",
        description: "Feature mode",
        env: "APP_MODE",
      }),
    })
    const stack = new alien.Stack("gated-stack")
      .inputs(io)
      .permissions({
        profiles: { proxy: { "*": [alien.permission("queue/data-write").enabled(io.mode, "on")] } },
      })
      .build()

    expect(stack.permissions?.gates).toEqual([
      {
        profile: "proxy",
        resource: "*",
        permissionSetId: "queue/data-write",
        inputId: "mode",
        enabledValue: "on",
      },
    ])
  })

  it("builds a valid integer gate from a numeric enabled value", () => {
    const io = alien.inputs({
      replicas: alien.integer({
        providedBy: "deployer",
        required: false,
        label: "Replicas",
        description: "Replica count",
        env: "APP_REPLICAS",
      }),
    })
    const stack = new alien.Stack("gated-stack")
      .inputs(io)
      .permissions({
        profiles: {
          proxy: { "*": [alien.permission("queue/data-write").enabled(io.replicas, 3)] },
        },
      })
      .build()

    expect(stack.permissions?.gates?.[0]?.enabledValue).toBe("3")
  })
})

describe("Build resource configuration", () => {
  it("creates a build with all configuration options", () => {
    // Create dependencies
    const registry = new alien.ArtifactRegistry("test-registry").build()
    const storage = new alien.Storage("test-storage").build()

    // Create build with all options
    const build = new alien.Build("comprehensive-build")
      .computeType("large")
      .environment({
        NODE_ENV: "production",
        BUILD_TARGET: "release",
        CUSTOM_VAR: "test-value",
      })
      .link(registry)
      .link(storage)
      .permissions("builder")
      .build()

    // Verify configuration
    expect(build.config.id).toBe("comprehensive-build")
    expect(build.config.computeType).toBe("large")
    expect(build.config.environment).toEqual({
      NODE_ENV: "production",
      BUILD_TARGET: "release",
      CUSTOM_VAR: "test-value",
    })
    expect(build.config.links).toHaveLength(2)
    expect(build.config.permissions).toBe("builder")

    expect(build).toMatchSnapshot()
  })

  it("creates a minimal build with defaults", () => {
    const build = new alien.Build("minimal-build").permissions("default").build()

    // Verify minimal configuration
    expect(build.config.id).toBe("minimal-build")
    expect(build.config.links).toEqual([])
    expect(build.config.environment).toEqual({})
    expect(build.config.computeType).toBeUndefined()
    expect(build.config.permissions).toBe("default")

    expect(build).toMatchSnapshot()
  })

  it("tests all compute types", () => {
    const computeTypes = ["small", "medium", "large", "x-large"] as const

    for (const computeType of computeTypes) {
      const build = new alien.Build(`build-${computeType}`)
        .computeType(computeType)
        .permissions("default")
        .build()

      expect(build.config.computeType).toBe(computeType)
    }
  })
})

describe("ArtifactRegistry resource configuration", () => {
  it("creates an artifact registry", () => {
    const registry = new alien.ArtifactRegistry("test-registry").build()

    // Verify configuration
    expect(registry.config.id).toBe("test-registry")

    expect(registry).toMatchSnapshot()
  })

  it("can be used in stack permissions", () => {
    const registry = new alien.ArtifactRegistry("protected-registry").build()

    const worker = new alien.Worker("registry-user")
      .code({ type: "image", image: SHARED_IMAGE })
      .permissions("execution")
      .build()

    const stack = new alien.Stack("registry-stack")
      .add(registry, "frozen")
      .add(worker, "live")
      .permissions({
        profiles: {
          execution: {
            "protected-registry": ["artifact-registry/data-read", "artifact-registry/data-write"],
          },
        },
      })
      .build()

    // Verify the stack includes permissions for the registry
    expect(stack.permissions?.profiles.execution?.["protected-registry"]).toEqual([
      "artifact-registry/data-read",
      "artifact-registry/data-write",
    ])

    expect(stack).toMatchSnapshot()
  })
})
