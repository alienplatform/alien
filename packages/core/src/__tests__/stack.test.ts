import { describe, expect, it } from "vitest"

import { FunctionSchema } from "../generated/index.js"
import * as alien from "../index.js"

// Shared image URI for functions in tests
const SHARED_IMAGE = "docker.io/library/rust:latest"

describe("Stack builder validation", () => {
  it("builds and validates a complex stack with permissions", () => {
    // Storage bucket
    const storage = new alien.Storage("my-test-bucket").publicRead(true).build()

    // Main application function with permissions
    const func = new alien.Function("my-test-function")
      .code({ type: "image", image: SHARED_IMAGE })
      .memoryMb(512)
      .timeoutSeconds(30)
      .permissions("execution")
      .ingress("public")
      .environment({
        RUST_LOG: "info,alien_runtime_test_server=debug,alien_runtime=debug",
      })
      .link(storage)
      .build()

    const stack = new alien.Stack("my-test-stack")
      .add(storage, "frozen")
      .add(func, "live")
      .permissions({
        profiles: {
          execution: {
            "*": ["storage/data-read"],
            "my-test-bucket": ["storage/data-write"],
          },
        },
        management: {
          extend: {
            "*": ["function/management", "storage/management"],
          },
        },
      })
      .build()

    // Basic assertions
    expect(stack.id).toBe("my-test-stack")
    expect(stack.resources).toHaveProperty("my-test-bucket")
    expect(stack.resources).toHaveProperty("my-test-function")
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

  it("builds and validates a stack with function source", () => {
    const funcWithSource = new alien.Function("my-source-function")
      .code({
        type: "source",
        src: "./app",
        toolchain: { type: "typescript" },
      })
      .memoryMb(256)
      .timeoutSeconds(15)
      .ingress("private")
      .permissions("execution")
      .build()

    const stack = new alien.Stack("my-source-stack")
      .add(funcWithSource, "live")
      .permissions({
        profiles: {
          execution: {
            "*": ["function/execute"],
          },
        },
        management: {
          extend: {
            "*": ["function/management"],
          },
        },
      })
      .build()

    expect(stack.id).toBe("my-source-stack")
    expect(stack.resources).toHaveProperty("my-source-function")
    const resourceInStack = stack.resources["my-source-function"]
    expect(resourceInStack).toBeDefined()
    const functionConfigFromStack = FunctionSchema.parse(resourceInStack!.config)
    expect(functionConfigFromStack.code.type).toBe("source")

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

    // Create a function with permissions
    const func = new alien.Function("test-function")
      .code({ type: "image", image: SHARED_IMAGE })
      .permissions("execution")
      .build()

    // Create stack with both string and custom permission sets
    const stack = new alien.Stack("permissions-stack")
      .add(func, "live")
      .permissions({
        profiles: {
          execution: {
            "*": ["storage/data-read", customPermissionSet],
          },
        },
        management: {
          extend: {
            "*": ["function/management"],
          },
        },
      })
      .build()

    // Verify the stack is properly configured
    expect(stack.id).toBe("permissions-stack")
    expect(stack.resources).toHaveProperty("test-function")
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

    const func = new alien.Function("registry-user")
      .code({ type: "image", image: SHARED_IMAGE })
      .permissions("execution")
      .build()

    const stack = new alien.Stack("registry-stack")
      .add(registry, "frozen")
      .add(func, "live")
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
