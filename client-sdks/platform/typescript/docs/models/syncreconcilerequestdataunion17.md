# SyncReconcileRequestDataUnion17


## Supported Types

### `models.DataAwsKms`

```typescript
const value: models.DataAwsKms = {
  data: {
    enabled: true,
    keyArn: "<value>",
    keySpec: "<value>",
    keyState: "<value>",
    keyUsage: "<value>",
    status: {
      health: "unhealthy",
      lifecycle: "stopped",
    },
  },
  provider: "aws-kms",
};
```

### `models.DataGcpCloudKms`

```typescript
const value: models.DataGcpCloudKms = {
  data: {
    cryptoKeyName: "<value>",
    purpose: "<value>",
    status: {
      health: "degraded",
      lifecycle: "running",
    },
  },
  provider: "gcp-cloud-kms",
};
```

### `models.DataAzureKeyVault2`

```typescript
const value: models.DataAzureKeyVault2 = {
  data: {
    keyId: "<id>",
    keyOperations: [],
    keyType: "<value>",
    status: {
      health: "unknown",
      lifecycle: "deleting",
    },
  },
  provider: "azure-key-vault",
};
```
