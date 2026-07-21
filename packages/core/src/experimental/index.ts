/**
 * Experimental resources.
 *
 * Resources under `alien.experimental` are provider-specific (they do not
 * abstract over clouds), are only registered for the platforms they support,
 * and may change in breaking ways before being promoted to portable
 * resources. Their resource type identifiers live under the `experimental/`
 * namespace (e.g. `experimental/aws-opensearch`).
 */
export * from "./aws-opensearch.js"
