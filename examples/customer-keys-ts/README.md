# Customer-controlled encryption keys

This example exposes a customer-managed key to an external application through Alien remote
bindings. The application encrypts and decrypts data without receiving the cloud key itself.

The encryption context binds ciphertext to a customer identifier. Decryption with a different
context fails, which helps prevent ciphertext from being moved between tenants.

## Run the stack locally

```bash
pnpm install
alien dev
```

For a remote deployment, create a deployment-scoped API token and run the vendor-side example:

```bash
export ALIEN_DEPLOYMENT_ID=dep_...
export ALIEN_API_TOKEN=...
export CUSTOMER_ID=customer_123
pnpm run run:vendor
```

Keep the deployment token in a secret manager and scope it to the deployment that owns the key.
