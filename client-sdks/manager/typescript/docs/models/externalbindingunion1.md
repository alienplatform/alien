# ExternalBindingUnion1

External storage binding (S3-compatible, GCS, Blob Storage)


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
  accountName: "<value>",
  containerName: "<value>",
  service: "blob",
  type: "storage",
};
```

### `models.ExternalBindingGcs`

```typescript
const value: models.ExternalBindingGcs = {
  bucketName: "<value>",
  service: "gcs",
  type: "storage",
};
```

### `models.ExternalBindingLocalStorage`

```typescript
const value: models.ExternalBindingLocalStorage = {
  storagePath: "<value>",
  service: "local-storage",
  type: "storage",
};
```
