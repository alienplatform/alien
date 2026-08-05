# Bring Your Own Bucket

Provision a dedicated object-storage resource in each customer's AWS, GCP, or
Azure account, then access it from your existing SaaS backend. No Worker,
Container, sidecar, or other application compute runs in the customer's cloud.

## Vendor: declare and release the storage

[`alien.ts`](./alien.ts) declares one Frozen Storage resource and opts it into
Remote Bindings:

```ts
const uploads = new alien.Storage("uploads").build()

export default new alien.Stack("byob-storage")
  .add(uploads, "frozen", { remoteAccess: true })
  .build()
```

Publish the release through the normal Alien release flow. `remoteAccess` is an
explicit security choice: it causes customer setup to grant the stack's separate
Remote Bindings identity object read, write, list, delete, and multipart access
to this dedicated bucket or container.

## Customer: run the normal setup

The customer creates a deployment from that release and chooses the setup method
that fits their environment:

- run `alien deploy` with their cloud credentials;
- apply the generated Terraform module; or
- on AWS, create the generated CloudFormation stack.

Every method creates one dedicated S3 bucket, GCS bucket, or Blob container and
one Remote Bindings identity. That identity trusts the vendor's configured
identity and receives only the object-storage permissions listed in the generated
`PERMISSIONS.md` (or visible directly in the CloudFormation template).

The generated setup registers the completed Frozen resources with Alien. Direct
setup performs the same work itself with the customer's setup credentials.

This flow does not attach an existing bucket. The resource must reach Running
before Remote Bindings can resolve it.

## Vendor backend: use the storage

Create an Alien API credential with write access to the deployment and keep it
only in trusted backend code. Set the deployment ID and credential in the
backend environment, then run the complete example in
[`src/vendor.ts`](./src/vendor.ts):

```sh
export ALIEN_DEPLOYMENT_ID=dep_...
export ALIEN_API_TOKEN=ax_...
pnpm run run:vendor
```

The application constructs one `Bindings` object and uses the ordinary Storage
operations:

```ts
const bindings = await Bindings.forRemoteDeployment({
  deploymentId: process.env.ALIEN_DEPLOYMENT_ID!,
  token: process.env.ALIEN_API_TOKEN!,
})

const uploads = bindings.storage("uploads")
await uploads.put("hello.txt", new TextEncoder().encode("hello"))
const object = await uploads.get("hello.txt")
const head = await uploads.head("hello.txt")
console.log(new TextDecoder().decode(object.data), head.meta, head.attributes)
await uploads.list()
await uploads.delete("hello.txt")
```

Provider credentials are short-lived and use the stack's Remote Bindings
identity, whose permissions are the union of resources explicitly opted into
remote access. The same `Bindings` and Storage objects refresh them below the
application API. Read-only or mismatched Alien credentials, non-Running
resources, and resources without `remoteAccess` are denied before usable cloud
credentials are returned.

Never expose the Alien API credential or returned provider credentials to a
browser, mobile app, logs, or other untrusted client.
