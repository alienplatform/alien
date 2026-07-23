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
explicit security choice: it causes customer setup to grant Alien's deployment
management identity object read, write, list, delete, and multipart access to
this dedicated bucket or container.

## Customer: run the normal setup

The customer creates a deployment from that release and completes the normal
generated CloudFormation, Terraform, or Azure setup. The setup creates a new
dedicated S3 bucket, GCS bucket, or Blob container in the customer's account and
hands the resulting Frozen resource state back to Alien.

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
await uploads.get("hello.txt")
await uploads.head("hello.txt")
await uploads.list()
await uploads.delete("hello.txt")
```

Provider credentials are short-lived and resource-scoped. The same `Bindings`
and Storage objects refresh them below the application API. Read-only or
mismatched Alien credentials, non-Running resources, and resources without
`remoteAccess` are denied before usable cloud credentials are returned.

Never expose the Alien API credential or returned provider credentials to a
browser, mobile app, logs, or other untrusted client.
