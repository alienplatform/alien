/**
 * Postgres binding accessor.
 *
 * Postgres is connection-only: unlike Storage/Kv/Queue there is no gRPC service to call,
 * so this module does not use {@link AlienContext}. Instead the manager injects the
 * connection details for a linked database into `ALIEN_<NAME>_BINDING` as JSON, and the
 * workload turns them into a live connection here.
 *
 * The cloud variants deliberately carry only a *reference* to the connection password
 * (Secrets Manager ARN / Secret Manager name / Key Vault URI), never the password itself.
 * The workload resolves that reference at runtime using its own least-privilege identity
 * — exactly what the `postgres/data-access` permission set grants — so the password never
 * flows through the control plane or sits as a plaintext environment variable.
 *
 * The matching cloud secret SDK is imported lazily, so a workload only loads the SDK for
 * the cloud it is actually running on.
 */

import { AlienError } from "@alienplatform/core"
import * as z from "zod/v4"
import {
  BindingNotFoundError,
  InvalidBindingConfigError,
  PostgresSecretResolutionError,
} from "../errors.js"

/**
 * Everything a Postgres driver needs to connect. `ssl` is the node-postgres TLS value; for
 * node-postgres pass it with the individual fields, not the connectionString (see the `ssl` note).
 * It is a plain value any driver can read.
 */
export interface PostgresConnection {
  /**
   * `postgres://user:password@host:port/database?sslmode=<mode>`, with credentials
   * percent-encoded. The `sslmode` mirrors the Rust resolver byte-for-byte (`disable`
   * local, `prefer` external, `require` cloud) so a sslmode-aware consumer agrees with us.
   */
  connectionString: string
  /**
   * The TLS setting for the driver: `false` for plaintext-capable bindings (local/external),
   * `{ rejectUnauthorized: false }` for managed clouds (TLS on, cert not yet verified against a
   * pinned CA).
   *
   * For node-postgres, pass this with the individual fields — `new Client({ host, port, ..., ssl })`
   * — NOT with the connectionString. node-postgres parses the URL's `sslmode` (it treats `require`
   * as `verify-full`) and that overwrites an explicit `ssl`, so the URL form rejects the cloud cert
   * with "unable to get local issuer certificate". Field style lets this value take effect.
   *
   * `rejectUnauthorized: false` is a deliberate v1 posture: Postgres is private-only on every cloud
   * (no public IP, reachable only from same-stack workloads), so the network boundary is the primary
   * control and an on-path MITM would already be inside the stack VPC. Verified TLS (provider CAs) is
   * future defense-in-depth, not a v1 requirement.
   *
   * For the `external` (BYO) backend this is always `false`: node-postgres has no `prefer` mode, so
   * the field stays plaintext while the connection string's `sslmode=prefer` only reaches sslmode-aware
   * consumers (psql). A BYO database that *requires* TLS must have TLS configured outside this struct.
   */
  ssl: false | { rejectUnauthorized: boolean }
  /**
   * The same connection details as individual fields, for drivers that take them separately
   * (`new Client({ host, port, user: username, password, database })`) instead of a URL. These are
   * the un-encoded values that `connectionString` percent-encodes; `host` is the address to dial
   * (the Aurora cluster endpoint for the managed-AWS backend).
   */
  host: string
  port: number
  database: string
  username: string
  password: string
}

const connectionFields = {
  // Fail fast at the trust boundary: reject any value that can't form a connectable URL rather than
  // pass it on. Port 0 is never dialable (the Rust `u16` domain allows it, so `.min(1)` is one
  // stricter); `database`/`username` likewise reject the empty string (Postgres has no empty
  // identifier, and `postgres://:p@h/` is unconnectable).
  port: z.number().int().min(1).max(65535),
  database: z.string().min(1),
  username: z.string().min(1),
}

/**
 * Mirror of the Rust `PostgresBinding` enum (tagged by `service`, camelCase fields).
 * `BindingValue<T>` is serialized untagged, so concrete values arrive as plain scalars.
 */
// `.strict()` on every variant: this binding is the control-plane → workload trust boundary, so an
// unexpected key (a Rust-side rename the SDK doesn't yet know, or env tampering) should fail loudly
// here rather than being silently dropped. This is the template other connection bindings will copy.
const postgresBindingSchema = z.discriminatedUnion("service", [
  z
    .object({
      service: z.literal("aurora"),
      clusterEndpoint: z.string(),
      passwordSecretArn: z.string(),
      ...connectionFields,
    })
    .strict(),
  z
    .object({
      service: z.literal("cloud-sql"),
      host: z.string(),
      passwordSecretName: z.string(),
      ...connectionFields,
    })
    .strict(),
  z
    .object({
      service: z.literal("flexible-server"),
      host: z.string(),
      passwordSecretUri: z.string(),
      ...connectionFields,
    })
    .strict(),
  z
    .object({
      service: z.literal("external"),
      host: z.string(),
      password: z.string().min(1),
      ...connectionFields,
    })
    .strict(),
  z
    .object({
      service: z.literal("local-postgres"),
      host: z.string(),
      password: z.string().min(1),
      ...connectionFields,
    })
    .strict(),
])

/** Matches the Rust `binding_env_var_name`: `ALIEN_<NAME>_BINDING`, hyphens to underscores. */
function bindingEnvVarName(bindingName: string): string {
  return `ALIEN_${bindingName.replace(/-/g, "_").toUpperCase()}_BINDING`
}

/** Mirrors the Rust `SslMode` query param so the two resolvers emit identical URLs. */
type SslMode = "disable" | "prefer" | "require"

// Percent-encode a userinfo/path component to the RFC 3986 *unreserved* set only, byte-for-byte
// matching the Rust resolver's `encode_userinfo`. `encodeURIComponent` leaves the sub-delims
// ! ' ( ) * literal; encode them too so a password containing any of them produces the same
// connection string on the TS and Rust runtimes (a generated password can include them).
export function encodeUserinfo(value: string): string {
  return encodeURIComponent(value).replace(
    /[!'()*]/g,
    c => `%${c.charCodeAt(0).toString(16).toUpperCase().padStart(2, "0")}`,
  )
}

function connectionString(
  host: string,
  params: { port: number; database: string; username: string },
  password: string,
  sslmode: SslMode,
): string {
  const user = encodeUserinfo(params.username)
  const pass = encodeUserinfo(password)
  return `postgres://${user}:${pass}@${host}:${params.port}/${encodeUserinfo(params.database)}?sslmode=${sslmode}`
}

/** Builds the full connection result — the URL plus the same details as individual fields. */
function makeConnection(
  host: string,
  params: { port: number; database: string; username: string },
  password: string,
  sslmode: SslMode,
  ssl: PostgresConnection["ssl"],
): PostgresConnection {
  return {
    connectionString: connectionString(host, params, password, sslmode),
    ssl,
    host,
    port: params.port,
    database: params.database,
    username: params.username,
    password,
  }
}

/**
 * Resolves the connection details for a linked Postgres database.
 *
 * @param bindingName The resource id used when the database was linked (e.g. the value
 *   passed to `new alien.Postgres(id)`).
 * @returns A connection string plus `ssl`, and the same details as individual fields
 *   (`host`/`port`/`database`/`username`/`password`) — use whichever style your driver prefers.
 * @throws AlienError if the binding env var is missing, malformed, or names a backend
 *   whose secret resolution is not available in this build.
 *
 * @example
 * ```typescript
 * import { Client } from "pg"
 * import { getPostgresConnection } from "@alienplatform/sdk"
 *
 * const conn = await getPostgresConnection("my-db")
 * // Field style: node-postgres parses a URL's sslmode and would override `ssl`, so pass fields.
 * const client = new Client({
 *   host: conn.host, port: conn.port, database: conn.database,
 *   user: conn.username, password: conn.password, ssl: conn.ssl,
 * })
 * await client.connect()
 * ```
 */
export async function getPostgresConnection(bindingName: string): Promise<PostgresConnection> {
  const envVar = bindingEnvVarName(bindingName)
  const raw = process.env[envVar]
  if (!raw) {
    // A missing binding env var means the workload didn't link a Postgres by this name — a
    // user-fixable config problem (404), not an internal failure.
    throw new AlienError(
      BindingNotFoundError.create({
        bindingName,
        bindingType: "Postgres",
      }),
    )
  }

  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch (cause) {
    throw (await AlienError.from(cause)).withContext(
      InvalidBindingConfigError.create({
        message: `Postgres binding '${bindingName}' is not valid JSON`,
      }),
    )
  }

  const result = postgresBindingSchema.safeParse(parsed)
  if (!result.success) {
    // Carry the Zod detail only in the wrapped internal source; the user-facing
    // InvalidBindingConfigError is `internal:false` and would leak it externally.
    throw (await AlienError.from(result.error)).withContext(
      InvalidBindingConfigError.create({
        message: `Postgres binding '${bindingName}' has an unexpected shape`,
      }),
    )
  }

  const binding = result.data
  switch (binding.service) {
    case "local-postgres":
      return makeConnection(binding.host, binding, binding.password, "disable", false)
    case "external":
      // node-postgres has no `prefer` mode: `ssl: false` always connects plaintext. We carry
      // `sslmode=prefer` in the URL for sslmode-aware consumers (psql), so against a server that
      // offers-but-doesn't-require TLS the two diverge (psql upgrades, pg stays plaintext) — an
      // accepted v1 downgrade; a BYO database requiring TLS needs explicit config.
      return makeConnection(binding.host, binding, binding.password, "prefer", false)
    case "aurora": {
      const password = await readAwsSecret(binding.passwordSecretArn)
      return makeConnection(binding.clusterEndpoint, binding, password, "require", {
        rejectUnauthorized: false,
      })
    }
    case "cloud-sql": {
      const password = await readGcpSecret(binding.passwordSecretName)
      return makeConnection(binding.host, binding, password, "require", {
        rejectUnauthorized: false,
      })
    }
    case "flexible-server": {
      const password = await readAzureSecret(binding.passwordSecretUri)
      return makeConnection(binding.host, binding, password, "require", {
        rejectUnauthorized: false,
      })
    }
    default:
      return assertNever(binding)
  }
}

/** Reads the raw password the AWS controller stored as the secret's `SecretString`. */
async function readAwsSecret(secretArn: string): Promise<string> {
  // Literal (not computed) import specifier so a bundler can leave it external — see the module
  // doc for why each cloud's secret SDK is loaded lazily.
  let sdk: typeof import("@aws-sdk/client-secrets-manager")
  try {
    sdk = await import("@aws-sdk/client-secrets-manager")
  } catch (cause) {
    throw (await AlienError.from(cause)).withContext(
      InvalidBindingConfigError.create({
        message: "Failed to load '@aws-sdk/client-secrets-manager'",
        suggestion: "Add '@aws-sdk/client-secrets-manager' to this workload's dependencies",
      }),
    )
  }
  const client = new sdk.SecretsManagerClient({})
  let password: string | undefined
  try {
    const response = await client.send(new sdk.GetSecretValueCommand({ SecretId: secretArn }))
    password = response.SecretString
  } catch (cause) {
    // A failed read is an upstream/transient failure (throttle, network, service unavailable), not
    // a user-fixable config error — make it retryable so an automated retry layer can recover.
    throw (await AlienError.from(cause)).withContext(
      PostgresSecretResolutionError.create({
        secret: secretArn,
        reason: "Secrets Manager GetSecretValue failed",
      }),
    )
  }
  if (!password) {
    // An empty stored secret is a control-plane invariant the workload can't fix, not bad user
    // input — surface it as a (retryable) resolution failure, like the read-failure path above.
    throw new AlienError(
      PostgresSecretResolutionError.create({
        secret: secretArn,
        reason: "Secrets Manager secret has no SecretString",
      }),
    )
  }
  return password
}

/** Reads the raw password the GCP controller stored as the secret version's payload. */
async function readGcpSecret(secretName: string): Promise<string> {
  let sdk: typeof import("@google-cloud/secret-manager")
  try {
    sdk = await import("@google-cloud/secret-manager")
  } catch (cause) {
    throw (await AlienError.from(cause)).withContext(
      InvalidBindingConfigError.create({
        message: "Failed to load '@google-cloud/secret-manager'",
        suggestion: "Add '@google-cloud/secret-manager' to this workload's dependencies",
      }),
    )
  }
  // `fallback: true` forces the REST/HTTPS transport instead of the default native gRPC
  // (@grpc/grpc-js over HTTP/2). Workloads run on Bun, whose HTTP/2 support doesn't carry gRPC
  // reliably: the gRPC stub init rejects and the first `accessSecretVersion` re-throws it
  // synchronously (no network call). REST matches the HTTP transport the AWS/Azure SDKs already use.
  const client = new sdk.SecretManagerServiceClient({ fallback: true })
  let data: Uint8Array | string | null | undefined
  try {
    const projectId = await client.getProjectId()
    const [version] = await client.accessSecretVersion({
      name: `projects/${projectId}/secrets/${secretName}/versions/latest`,
    })
    data = version.payload?.data
  } catch (cause) {
    throw (await AlienError.from(cause)).withContext(
      PostgresSecretResolutionError.create({
        secret: secretName,
        reason: "Secret Manager accessSecretVersion failed",
      }),
    )
  }
  // `!data` does not catch a zero-length Uint8Array (an empty typed array is truthy), which would
  // otherwise decode to an empty password and silently pass. Guard on length so an empty payload
  // fails fast like the AWS/Azure paths (`!password` / `!value`).
  if (data == null || data.length === 0) {
    // An empty stored secret is a control-plane invariant the workload can't fix, not bad user
    // input — surface it as a (retryable) resolution failure, like the read-failure path above.
    throw new AlienError(
      PostgresSecretResolutionError.create({
        secret: secretName,
        reason: "Secret Manager secret has no payload",
      }),
    )
  }
  return Buffer.from(data).toString("utf8")
}

/**
 * Reads the raw password the Azure controller stored as a Key Vault secret. `secretUri` is the full
 * Key Vault secret URI the binding carries:
 * `https://<vault>.vault.azure.net/secrets/<name>[/<version>]`.
 */
async function readAzureSecret(secretUri: string): Promise<string> {
  let url: URL
  try {
    url = new URL(secretUri)
  } catch (cause) {
    throw (await AlienError.from(cause)).withContext(
      InvalidBindingConfigError.create({
        message: `Postgres password secret URI '${secretUri}' is not a valid URL`,
      }),
    )
  }
  // Path is exactly `/secrets/<name>[/<version>]`; a missing version means "latest". Reject extra
  // segments rather than silently ignore them, matching the schema's fail-loud posture.
  const segments = url.pathname.split("/").filter(Boolean)
  const secretName = segments[1]
  const secretVersion = segments[2]
  if (segments[0] !== "secrets" || !secretName || segments.length > 3) {
    throw new AlienError(
      InvalidBindingConfigError.create({
        message: `Key Vault secret URI '${secretUri}' is not a '/secrets/<name>' URL`,
      }),
    )
  }

  let secrets: typeof import("@azure/keyvault-secrets")
  let identity: typeof import("@azure/identity")
  try {
    secrets = await import("@azure/keyvault-secrets")
    identity = await import("@azure/identity")
  } catch (cause) {
    throw (await AlienError.from(cause)).withContext(
      InvalidBindingConfigError.create({
        message: "Failed to load '@azure/keyvault-secrets' / '@azure/identity'",
        suggestion:
          "Add '@azure/keyvault-secrets' and '@azure/identity' to this workload's dependencies",
      }),
    )
  }
  const client = new secrets.SecretClient(url.origin, new identity.DefaultAzureCredential())
  let value: string | undefined
  try {
    const secret = await client.getSecret(
      secretName,
      secretVersion ? { version: secretVersion } : undefined,
    )
    value = secret.value
  } catch (cause) {
    throw (await AlienError.from(cause)).withContext(
      PostgresSecretResolutionError.create({
        secret: secretName,
        reason: "Key Vault getSecret failed",
      }),
    )
  }
  if (!value) {
    // An empty stored secret is a control-plane invariant the workload can't fix, not bad user
    // input — surface it as a (retryable) resolution failure, like the read-failure path above.
    throw new AlienError(
      PostgresSecretResolutionError.create({
        secret: secretName,
        reason: "Key Vault secret has no value",
      }),
    )
  }
  return value
}

function assertNever(_value: never): never {
  // Don't serialize the value: the external/local variants carry a plaintext password, and this
  // error is `internal: false`, so its message can surface to API clients.
  throw new AlienError(
    InvalidBindingConfigError.create({
      message: "Postgres binding has an unhandled service variant",
    }),
  )
}
