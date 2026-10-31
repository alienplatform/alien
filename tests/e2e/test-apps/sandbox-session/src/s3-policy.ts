// Reads a bucket's policy and writes it back unchanged, signed with this Worker's own role.
// Bun's S3 client covers objects only, so the two bucket-policy calls are signed here (AWS
// Signature Version 4).

const encoder = new TextEncoder()

async function sha256Hex(data: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", encoder.encode(data))
  return Buffer.from(digest).toString("hex")
}

async function hmac(key: ArrayBuffer | Uint8Array, data: string): Promise<ArrayBuffer> {
  const cryptoKey = await crypto.subtle.importKey(
    "raw",
    key,
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  )
  return crypto.subtle.sign("HMAC", cryptoKey, encoder.encode(data))
}

async function signedPolicyRequest(
  method: "GET" | "PUT",
  bucket: string,
  body: string,
): Promise<Response> {
  const region = process.env.AWS_REGION ?? ""
  const host = `${bucket}.s3.${region}.amazonaws.com`
  const now = new Date().toISOString().replace(/[:-]|\.\d{3}/g, "")
  const date = now.slice(0, 8)
  const payloadHash = await sha256Hex(body)
  const headers: Record<string, string> = {
    host,
    "x-amz-content-sha256": payloadHash,
    "x-amz-date": now,
    "x-amz-security-token": process.env.AWS_SESSION_TOKEN ?? "",
  }
  const signedHeaders = Object.keys(headers).sort().join(";")
  const canonicalHeaders = Object.keys(headers)
    .sort()
    .map(name => `${name}:${headers[name]}\n`)
    .join("")
  const canonicalRequest = [
    method,
    "/",
    "policy=",
    canonicalHeaders,
    signedHeaders,
    payloadHash,
  ].join("\n")
  const scope = `${date}/${region}/s3/aws4_request`
  const stringToSign = ["AWS4-HMAC-SHA256", now, scope, await sha256Hex(canonicalRequest)].join(
    "\n",
  )
  let key: ArrayBuffer | Uint8Array = encoder.encode(`AWS4${process.env.AWS_SECRET_ACCESS_KEY}`)
  for (const part of [date, region, "s3", "aws4_request"]) key = await hmac(key, part)
  const signature = Buffer.from(await hmac(key, stringToSign)).toString("hex")
  return fetch(`https://${host}/?policy=`, {
    method,
    headers: {
      ...headers,
      authorization: `AWS4-HMAC-SHA256 Credential=${process.env.AWS_ACCESS_KEY_ID}/${scope}, SignedHeaders=${signedHeaders}, Signature=${signature}`,
    },
    body: method === "PUT" ? body : undefined,
  })
}

/** S3's error code from a failed response body, such as `AccessDenied`. */
async function errorCode(response: Response): Promise<string> {
  const text = await response.text()
  return /<Code>([^<]+)<\/Code>/.exec(text)?.[1] ?? `HTTP ${response.status}`
}

export async function rewriteBucketPolicy(
  bucket: string,
): Promise<{ rewritten: boolean; code?: string }> {
  const current = await signedPolicyRequest("GET", bucket, "")
  if (!current.ok) return { rewritten: false, code: await errorCode(current) }
  const written = await signedPolicyRequest("PUT", bucket, await current.text())
  return written.ok ? { rewritten: true } : { rewritten: false, code: await errorCode(written) }
}
