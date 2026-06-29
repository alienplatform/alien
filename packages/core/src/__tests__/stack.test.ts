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
          gpu: undefined,
        },
        minSize: 2,
        maxSize: 2,
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

    expect(stack.inputs).toHaveLength(3)
    expect(stack.inputs.map(input => input.id)).toEqual([
      "apiBaseUrl",
      "accessKey",
      "deploymentTier",
    ])
    expect(stack.inputs.find(input => input.id === "apiBaseUrl")).toMatchObject({
      kind: "string",
      providedBy: ["developer", "deployer"],
      required: true,
      validation: { format: "url" },
      env: [{ name: "API_BASE_URL" }],
    })
    expect(stack.inputs.find(input => input.id === "accessKey")).toMatchObject({
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
    expect(stack.inputs.find(input => input.id === "deploymentTier")).toMatchObject({
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
