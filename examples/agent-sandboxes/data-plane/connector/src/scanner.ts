// The scanner is reached through the connector, which adds the customer's own key. The sandbox
// never holds it, not even a placeholder.

/** Fetches a scanner path with the customer's key. Only reads: the route accepts GET. */
export async function scannerGet(
  baseUrl: string,
  key: string,
  path: string,
): Promise<{ status: number; body: unknown }> {
  if (!baseUrl || !key) return { status: 503, body: { error: "SCANNER_URL or the scanner API key is not set" } }
  const response = await fetch(`${baseUrl.replace(/\/$/, "")}${path}`, {
    headers: { authorization: `Bearer ${key}` },
  })
  return { status: response.status, body: await response.json() }
}
