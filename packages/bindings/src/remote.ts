import { unwrapNapiError } from "./errors.js"
import { createRemoteStorageFactory } from "./factories.js"
import { loadAddon } from "./loader.js"
import type { RemoteStorage } from "./types.js"

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

  private constructor(storage: (name: string) => RemoteStorage) {
    this.#storage = storage
  }

  /** Discover the deployment's manager and prepare remote Storage bindings. */
  static async forRemoteDeployment(options: RemoteDeploymentBindingsOptions): Promise<Bindings> {
    try {
      const addon = loadAddon()
      const bindings = await addon.BindingsHandle.forRemoteDeployment(
        options.deploymentId,
        options.token,
        options.apiBaseUrl,
      )
      return new Bindings(createRemoteStorageFactory(bindings))
    } catch (error) {
      throw unwrapNapiError(error)
    }
  }

  /** Resolve a remote Storage binding by resource name. */
  storage(name: string): RemoteStorage {
    return this.#storage(name)
  }
}
