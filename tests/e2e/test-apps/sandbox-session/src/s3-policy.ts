// Bucket-level calls signed with this Worker's own role: rewriting a bucket's policy unchanged,
// and deleting a bucket.
// Bun's S3 client covers objects only, so these calls are signed here (AWS Signature Version 4).

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

/** A signed request to `https://<host>/?<query>`; `query` is already in canonical order. */
async function signedRequest(
  service: "s3" | "sts",
  host: string,
  method: "GET" | "PUT" | "DELETE",
  query: string,
  body: string,
): Promise<Response> {
  const region = process.env.AWS_REGION ?? ""
  const now = new Date().toISOString().replace(/[:-]|\.\d{3}/g, "")
  const date = now.slice(0, 8)
  const payloadHash = await sha256Hex(body)
  const headers: Record<string, string> = {
    host,
    "x-amz-content-sha256": payloadHash,
    "x-amz-date": now,
    "x-amz-security-token": process.env.AWS_SESSION_TOKEN ?? "",
  }
  const names = Object.keys(headers).sort()
  const signedHeaders = names.join(";")
  const canonicalHeaders = names.map(name => `${name}:${headers[name]}\n`).join("")
  const canonicalRequest = [method, "/", query, canonicalHeaders, signedHeaders, payloadHash].join(
    "\n",
  )
  const scope = `${date}/${region}/${service}/aws4_request`
  const stringToSign = ["AWS4-HMAC-SHA256", now, scope, await sha256Hex(canonicalRequest)].join(
    "\n",
  )
  let key: ArrayBuffer | Uint8Array = encoder.encode(`AWS4${process.env.AWS_SECRET_ACCESS_KEY}`)
  for (const part of [date, region, service, "aws4_request"]) key = await hmac(key, part)
  const signature = Buffer.from(await hmac(key, stringToSign)).toString("hex")
  return fetch(`https://${host}/?${query}`, {
    method,
    headers: {
      ...headers,
      authorization: `AWS4-HMAC-SHA256 Credential=${process.env.AWS_ACCESS_KEY_ID}/${scope}, SignedHeaders=${signedHeaders}, Signature=${signature}`,
    },
    body: method === "PUT" ? body : undefined,
  })
}

function xmlValue(text: string, tag: string): string | undefined {
  return new RegExp(`<${tag}>([^<]+)</${tag}>`).exec(text)?.[1]
}

/** S3's error code from a failed response body, such as `AccessDenied`. */
async function errorCode(response: Response): Promise<string> {
  return xmlValue(await response.text(), "Code") ?? `HTTP ${response.status}`
}

/** The ARN this Worker signs in as: its role's assumed-role session. */
async function callerArn(): Promise<string | undefined> {
  const region = process.env.AWS_REGION ?? ""
  const response = await signedRequest(
    "sts",
    `sts.${region}.amazonaws.com`,
    "GET",
    "Action=GetCallerIdentity&Version=2011-06-15",
    "",
  )
  return xmlValue(await response.text(), "Arn")
}

export async function rewriteBucketPolicy(
  bucket: string,
): Promise<{ rewritten: boolean; code?: string; callerArn?: string }> {
  const host = `${bucket}.s3.${process.env.AWS_REGION}.amazonaws.com`
  const caller = await callerArn()
  const current = await signedRequest("s3", host, "GET", "policy=", "")
  if (!current.ok) return { rewritten: false, code: await errorCode(current), callerArn: caller }
  const written = await signedRequest("s3", host, "PUT", "policy=", await current.text())
  return written.ok
    ? { rewritten: true, callerArn: caller }
    : { rewritten: false, code: await errorCode(written), callerArn: caller }
}

/** Deletes the bucket. S3 refuses a bucket that still holds objects with `BucketNotEmpty`. */
export async function deleteBucket(bucket: string): Promise<{ deleted: boolean; code?: string }> {
  const host = `${bucket}.s3.${process.env.AWS_REGION}.amazonaws.com`
  const response = await signedRequest("s3", host, "DELETE", "", "")
  return response.ok ? { deleted: true } : { deleted: false, code: await errorCode(response) }
}
