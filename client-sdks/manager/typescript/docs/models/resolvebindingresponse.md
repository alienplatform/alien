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
      expiresAt: "1755867390141",
      secretAccessKey: "<value>",
      sessionToken: "<value>",
      type: "sessionCredentials",
    },
    region: "<value>",
  },
  expiresAt: "1750122153944",
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
      sas: {
        accountName: "<value>",
        containerName: "<value>",
        expiresAt: "1762181110811",
        permissions: "<value>",
        protocol: "<value>",
        serviceVersion: "<value>",
        signature: "<value>",
        signedKeyExpiry: "<value>",
        signedKeyService: "<value>",
        signedKeyStart: "<value>",
        signedKeyVersion: "<value>",
        signedObjectId: "<id>",
        signedResource: "<value>",
        signedTenantId: "<id>",
        startsAt: "<value>",
      },
      type: "containerSas",
    },
    subscriptionId: "<id>",
    tenantId: "<id>",
  },
  expiresAt: "1759301232953",
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
