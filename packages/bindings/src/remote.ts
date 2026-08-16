import { z } from "zod"
import { unwrapNapiError } from "./errors.js"
import { createRemoteKeyFactory, createRemoteStorageFactory } from "./factories.js"
import { loadAddon } from "./loader.js"
import type { Key, RemoteStorage } from "./types.js"

const aiBindingSchema = z.discriminatedUnion("service", [
  z.object({ service: z.literal("bedrock"), region: z.string().min(1) }),
  z.object({
    service: z.literal("vertex"),
    project: z.string().min(1),
    location: z.string().min(1),
  }),
  z.object({
    service: z.literal("foundry"),
    endpoint: z.url(),
    account: z.string().min(1),
  }),
])

const remoteClientConfigSchema = z.discriminatedUnion("platform", [
  z.object({ platform: z.literal("aws") }).passthrough(),
  z.object({ platform: z.literal("gcp") }).passthrough(),
  z.object({ platform: z.literal("azure") }).passthrough(),
])

export type RemoteAiBinding = z.infer<typeof aiBindingSchema>
export type RemoteAiClientConfig = z.infer<typeof remoteClientConfigSchema>

export interface RemoteAiLease {
  resourceId: string
  binding: RemoteAiBinding
  clientConfig: RemoteAiClientConfig
  expiresAt: Date
}

/** Options for accessing Storage resources in an existing deployment. */
export interface RemoteDeploymentBindingsOptions {
  /** Deployment to access. */
  deploymentId: string
  /** Alien API token authorized for remote bindings. */
  token: string
  /** Override the Alien API base URL. */
  apiBaseUrl?: string
}

/** Remote bindings for an existing deployment. */
export class Bindings {
  readonly #storage: (name: string) => RemoteStorage
  readonly #key: (name: string) => Key
  readonly #ai: () => Promise<RemoteAiLease>

  private constructor(
    storage: (name: string) => RemoteStorage,
    key: (name: string) => Key,
    ai: () => Promise<RemoteAiLease>,
  ) {
    this.#storage = storage
    this.#key = key
    this.#ai = ai
  }

  /** Discover the deployment's manager and prepare remote Storage bindings. */
  static async forRemoteDeployment(options: RemoteDeploymentBindingsOptions): Promise<Bindings> {
    try {
      const addon = loadAddon()
      const bindings = await addon.RemoteBindingsHandle.forDeployment(
        options.deploymentId,
        options.token,
        options.apiBaseUrl,
      )
      return new Bindings(
        createRemoteStorageFactory(bindings),
        createRemoteKeyFactory(bindings),
        async () => {
          const lease = await bindings.ai()
          return {
            resourceId: lease.resourceId,
            binding: aiBindingSchema.parse(JSON.parse(lease.bindingJson)),
            clientConfig: remoteClientConfigSchema.parse(JSON.parse(lease.clientConfigJson)),
            expiresAt: z.iso
              .datetime({ offset: true })
              .transform(value => new Date(value))
              .parse(lease.expiresAt),
          }
        },
      )
    } catch (error) {
      throw unwrapNapiError(error)
    }
  }

  /** Resolve a remote Storage binding by resource name. */
  storage(name: string): RemoteStorage {
    return this.#storage(name)
  }

  /** Resolve a remote Key binding by resource name. */
  key(name: string): Key {
    return this.#key(name)
  }

  /** Resolve the deployment's unique managed AI binding. */
  ai(): Promise<RemoteAiLease> {
    return this.#ai()
  }
}
