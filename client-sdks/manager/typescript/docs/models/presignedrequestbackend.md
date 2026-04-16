# PresignedRequestBackend

Storage backend representation for different presigned request types


## Supported Types

### `models.PresignedRequestBackendHTTP`

```typescript
const value: models.PresignedRequestBackendHTTP = {
  headers: {
    "key": "<value>",
    "key1": "<value>",
  },
  method: "<value>",
  type: "http",
  url: "https://cheerful-wafer.info",
};
```

### `models.PresignedRequestBackendLocal`

```typescript
const value: models.PresignedRequestBackendLocal = {
  filePath: "/Users/seafood.bz",
  operation: "get",
  type: "local",
};
```

