import { type WorkerProtocolGeneration, getGrpcEndpointConfig } from "./channel.js"
import { ControlServiceDefinition } from "./generated/control.js"
import { WaitUntilServiceDefinition } from "./generated/wait_until.js"

export type ControlService = Omit<typeof ControlServiceDefinition, "fullName"> & {
  readonly fullName: string
}

export type WaitUntilService = Omit<typeof WaitUntilServiceDefinition, "fullName"> & {
  readonly fullName: string
}

const LegacyControlServiceDefinition = {
  ...ControlServiceDefinition,
  fullName: "alien_bindings.control.ControlService",
} satisfies ControlService

const LegacyWaitUntilServiceDefinition = {
  ...WaitUntilServiceDefinition,
  fullName: "alien_bindings.wait_until.WaitUntilService",
} satisfies WaitUntilService

export function getControlServiceDefinition(
  generation: WorkerProtocolGeneration = getGrpcEndpointConfig().generation,
): ControlService {
  return generation === "legacy" ? LegacyControlServiceDefinition : ControlServiceDefinition
}

export function getWaitUntilServiceDefinition(
  generation: WorkerProtocolGeneration = getGrpcEndpointConfig().generation,
): WaitUntilService {
  return generation === "legacy" ? LegacyWaitUntilServiceDefinition : WaitUntilServiceDefinition
}
