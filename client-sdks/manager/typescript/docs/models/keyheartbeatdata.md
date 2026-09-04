# KeyHeartbeatData


## Supported Types

### `models.KeyHeartbeatDataAwsKms`

```typescript
const value: models.KeyHeartbeatDataAwsKms = {
  data: {
    enabled: true,
    keyArn: "<value>",
    keySpec: "<value>",
    keyState: "<value>",
    keyUsage: "<value>",
    status: {
      health: "healthy",
      lifecycle: "running",
    },
  },
  provider: "aws-kms",
};
```

### `models.KeyHeartbeatDataGcpCloudKms`

```typescript
const value: models.KeyHeartbeatDataGcpCloudKms = {
  data: {
    cryptoKeyName: "<value>",
    purpose: "<value>",
    status: {
      health: "healthy",
      lifecycle: "running",
    },
  },
  provider: "gcp-cloud-kms",
};
```

### `models.KeyHeartbeatDataAzureKeyVault`

```typescript
const value: models.KeyHeartbeatDataAzureKeyVault = {
  data: {
    keyId: "<id>",
    keyOperations: [
      "<value 1>",
    ],
    keyType: "<value>",
    status: {
      health: "healthy",
      lifecycle: "running",
    },
  },
  provider: "azure-key-vault",
};
```
