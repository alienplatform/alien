import * as z from "zod/v4"
import { ResourceNotFoundError, ResourceOutputsParseError } from "./common-errors.js"
import { AlienError } from "./error.js"
import {
  ArtifactRegistryOutputsSchema,
  ArtifactRegistrySchema,
  BuildOutputsSchema,
  BuildSchema,
  ContainerOutputsSchema,
  ContainerSchema,
  FunctionOutputsSchema,
  FunctionSchema,
  KvOutputsSchema,
  KvSchema,
  QueueOutputsSchema,
  QueueSchema,
  RemoteStackManagementOutputsSchema,
  RemoteStackManagementSchema,
  ServiceAccountOutputsSchema,
  ServiceAccountSchema,
  StorageOutputsSchema,
  StorageSchema,
  VaultOutputsSchema,
  VaultSchema,
} from "./generated/index.js"
import type { StackState } from "./stack.js"

export const ResourceSchemaMapping = {
  function: {
    input: FunctionSchema,
    output: FunctionOutputsSchema,
  },
  container: {
    input: ContainerSchema,
    output: ContainerOutputsSchema,
  },
  storage: {
    input: StorageSchema,
    output: StorageOutputsSchema,
  },
  kv: {
    input: KvSchema,
    output: KvOutputsSchema,
  },
  queue: {
    input: QueueSchema,
    output: QueueOutputsSchema,
  },
  vault: {
    input: VaultSchema,
    output: VaultOutputsSchema,
  },
  build: {
    input: BuildSchema,
    output: BuildOutputsSchema,
  },
  "artifact-registry": {
    input: ArtifactRegistrySchema,
    output: ArtifactRegistryOutputsSchema,
  },
  "service-account": {
    input: ServiceAccountSchema,
    output: ServiceAccountOutputsSchema,
  },
  "remote-stack-management": {
    input: RemoteStackManagementSchema,
    output: RemoteStackManagementOutputsSchema,
  },
}

// Retrieves and validates the outputs of a resource from the stack state.
export function getResourceOutputs<K extends keyof typeof ResourceSchemaMapping>(params: {
  state: StackState
  resource: { type: K; name: string }
}): z.infer<(typeof ResourceSchemaMapping)[K]["output"]> {
  const { state, resource } = params

  const resourceState = state.resources[resource.name]
  if (!resourceState) {
    throw new AlienError(
      ResourceNotFoundError.create({
        resourceId: resource.name,
        availableResources: Object.keys(state.resources),
      }),
    )
  }

  const outputsSchema = ResourceSchemaMapping[resource.type].output
  const outputs = outputsSchema.safeParse(resourceState.outputs)
  if (!outputs.success) {
    throw new AlienError(
      ResourceOutputsParseError.create({
        resourceName: resource.name,
        resourceType: resource.type,
        validationErrors: z.prettifyError(outputs.error),
      }),
    )
  }

  return outputs.data as z.infer<(typeof ResourceSchemaMapping)[K]["output"]>
}
