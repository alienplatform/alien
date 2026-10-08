# ExternalBindingUnion1

Service-type based storage binding that supports multiple storage providers


## Supported Types

### `models.ExternalBindingS3`

```typescript
const value: models.ExternalBindingS3 = {
  bucketName: "<value>",
  service: "s3",
  type: "storage",
};
```

### `models.ExternalBindingBlob`

```typescript
const value: models.ExternalBindingBlob = {
  accountName: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  containerName: "<value>",
  service: "blob",
  type: "storage",
};
```

### `models.ExternalBindingGcs`

```typescript
const value: models.ExternalBindingGcs = {
  bucketName: null,
  service: "gcs",
  type: "storage",
};
```

### `models.ExternalBindingLocalStorage`

```typescript
const value: models.ExternalBindingLocalStorage = {
  storagePath: null,
  service: "local-storage",
  type: "storage",
};
```

