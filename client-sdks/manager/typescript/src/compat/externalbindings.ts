import * as z from "zod/v4";
import { safeParse } from "../lib/schemas.js";
import {
  ExternalBindingUnion$inboundSchema,
  ExternalBindingUnion$outboundSchema,
} from "../models/externalbindingunion.js";

/** Backward-compatible name for the typed external binding map. */
export const ExternalBindings$inboundSchema = z.record(
  z.string(),
  ExternalBindingUnion$inboundSchema,
);
export const ExternalBindings$outboundSchema = z.record(
  z.string(),
  ExternalBindingUnion$outboundSchema,
);
export type ExternalBindings = z.infer<typeof ExternalBindings$inboundSchema>;
export type ExternalBindings$Outbound = z.output<typeof ExternalBindings$outboundSchema>;

export function externalBindingsToJSON(bindings: ExternalBindings): string {
  return JSON.stringify(ExternalBindings$outboundSchema.parse(bindings));
}

export function externalBindingsFromJSON(jsonString: string) {
  return safeParse(
    jsonString,
    (value) => ExternalBindings$inboundSchema.parse(JSON.parse(value)),
    "Failed to parse 'ExternalBindings' from JSON",
  );
}
