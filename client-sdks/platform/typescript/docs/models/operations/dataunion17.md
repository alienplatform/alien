# DataUnion17


## Supported Types

### `operations.DataAwsKms`

```typescript
const value: operations.DataAwsKms = {
  data: {
    enabled: true,
    keyArn: "<value>",
    keySpec: "<value>",
    keyState: "<value>",
    keyUsage: "<value>",
    status: {
      health: "unknown",
      lifecycle: "unknown",
    },
  },
  provider: "aws-kms",
};
```

### `operations.DataGcpCloudKms`

```typescript
const value: operations.DataGcpCloudKms = {
  data: {
    cryptoKeyName: "<value>",
    purpose: "<value>",
    status: {
      health: "degraded",
      lifecycle: "updating",
    },
  },
  provider: "gcp-cloud-kms",
};
```

### `operations.DataAzureKeyVault2`

```typescript
const value: operations.DataAzureKeyVault2 = {
  data: {
    keyId: "<id>",
    keyOperations: [],
    keyType: "<value>",
    status: {
      health: "unknown",
      lifecycle: "running",
    },
  },
  provider: "azure-key-vault",
};
```

