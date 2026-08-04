import { unwrapNapiError } from "./errors.js"
import { createRemoteKeyFactory, createRemoteStorageFactory } from "./factories.js"
import { loadAddon } from "./loader.js"
import type { Key, RemoteStorage } from "./types.js"

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

  private constructor(storage: (name: string) => RemoteStorage, key: (name: string) => Key) {
    this.#storage = storage
    this.#key = key
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
      return new Bindings(createRemoteStorageFactory(bindings), createRemoteKeyFactory(bindings))
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
}
