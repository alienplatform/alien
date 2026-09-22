# KeyHeartbeatData


## Supported Types

### `models.KeyHeartbeatDataAwsKms`

```typescript
const value: models.KeyHeartbeatDataAwsKms = {
  data: {
    enabled: false,
    keyArn: "<value>",
    keySpec: "<value>",
    keyState: "<value>",
    keyUsage: "<value>",
    status: {
      health: "unknown",
      lifecycle: "deleted",
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
      health: "unknown",
      lifecycle: "deleted",
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
      health: "unknown",
      lifecycle: "deleted",
    },
  },
  provider: "azure-key-vault",
};
```
