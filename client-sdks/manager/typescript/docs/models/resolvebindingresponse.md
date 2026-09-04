# ResolveBindingResponse

One approved remote Storage binding paired with credentials for the same
provider. The discriminant makes cross-provider combinations impossible.


## Supported Types

### `models.ResolveBindingResponseS3`

```typescript
const value: models.ResolveBindingResponseS3 = {
  binding: {
    bucketName: "<value>",
  },
  clientConfig: {
    accountId: "<id>",
    credentials: {
      accessKeyId: "<id>",
      expiresAt: "1744601542027",
      secretAccessKey: "<value>",
      sessionToken: "<value>",
      type: "sessionCredentials",
    },
    region: "<value>",
  },
  expiresAt: "1755867390141",
  service: "s3",
};
```

### `models.ResolveBindingResponseBlob`

```typescript
const value: models.ResolveBindingResponseBlob = {
  binding: {
    accountName: "<value>",
    containerName: "<value>",
  },
  clientConfig: {
    credentials: {
      token: "<value>",
      type: "accessToken",
    },
    subscriptionId: "<id>",
    tenantId: "<id>",
  },
  expiresAt: "1762181110811",
  service: "blob",
};
```

### `models.ResolveBindingResponseGcs`

```typescript
const value: models.ResolveBindingResponseGcs = {
  binding: {
    bucketName: "<value>",
  },
  clientConfig: {
    credentials: {
      token: "<value>",
      type: "accessToken",
    },
    projectId: "<id>",
    region: "<value>",
  },
  expiresAt: "1741179780880",
  service: "gcs",
};
```

### `models.ResolveBindingResponseKms`

```typescript
const value: models.ResolveBindingResponseKms = {
  binding: {
    keyArn: "<value>",
  },
  clientConfig: {
    accountId: "<id>",
    credentials: {
      accessKeyId: "<id>",
      expiresAt: "1744601542027",
      secretAccessKey: "<value>",
      sessionToken: "<value>",
      type: "sessionCredentials",
    },
    region: "<value>",
  },
  expiresAt: "1738330324847",
  service: "kms",
};
```

### `models.ResolveBindingResponseCloudKms`

```typescript
const value: models.ResolveBindingResponseCloudKms = {
  binding: {
    cryptoKeyName: "<value>",
  },
  clientConfig: {
    credentials: {
      token: "<value>",
      type: "accessToken",
    },
    projectId: "<id>",
    region: "<value>",
  },
  expiresAt: "1760734762170",
  service: "cloud-kms",
};
```

### `models.ResolveBindingResponseKeyVaultKey`

```typescript
const value: models.ResolveBindingResponseKeyVaultKey = {
  binding: {
    keyId: "<id>",
  },
  clientConfig: {
    credentials: {
      token: "<value>",
      type: "accessToken",
    },
    subscriptionId: "<id>",
    tenantId: "<id>",
  },
  expiresAt: "1740403538042",
  service: "key-vault-key",
};
```

### `models.ResolveBindingResponseBedrock`

```typescript
const value: models.ResolveBindingResponseBedrock = {
  binding: {
    region: "<value>",
  },
  clientConfig: {
    accountId: "<id>",
    credentials: {
      accessKeyId: "<id>",
      expiresAt: "1744601542027",
      secretAccessKey: "<value>",
      sessionToken: "<value>",
      type: "sessionCredentials",
    },
    region: "<value>",
  },
  expiresAt: "1764305704909",
  resourceId: "<id>",
  service: "bedrock",
};
```

### `models.ResolveBindingResponseVertex`

```typescript
const value: models.ResolveBindingResponseVertex = {
  binding: {
    location: "<value>",
    project: "<value>",
  },
  clientConfig: {
    credentials: {
      token: "<value>",
      type: "accessToken",
    },
    projectId: "<id>",
    region: "<value>",
  },
  expiresAt: "1739955288025",
  resourceId: "<id>",
  service: "vertex",
};
```

### `models.ResolveBindingResponseFoundry`

```typescript
const value: models.ResolveBindingResponseFoundry = {
  binding: {
    account: "76435551",
    endpoint: "<value>",
  },
  clientConfig: {
    credentials: {
      token: "<value>",
      type: "accessToken",
    },
    subscriptionId: "<id>",
    tenantId: "<id>",
  },
  expiresAt: "1750818839042",
  resourceId: "<id>",
  service: "foundry",
};
```

### `models.ResolveBindingResponseSandboxAws`

```typescript
const value: models.ResolveBindingResponseSandboxAws = {
  binding: {
    allowEgress: false,
    imageArn: "<value>",
    imageVersion: "<value>",
    previewPorts: [
      270838,
      279654,
      152578,
    ],
    region: "<value>",
  },
  clientConfig: {
    accountId: "<id>",
    credentials: {
      accessKeyId: "<id>",
      expiresAt: "1744601542027",
      secretAccessKey: "<value>",
      sessionToken: "<value>",
      type: "sessionCredentials",
    },
    region: "<value>",
  },
  expiresAt: "1755710658211",
  service: "sandbox-aws",
};
```

