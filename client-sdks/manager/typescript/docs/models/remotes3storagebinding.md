# RemoteS3StorageBinding

Concrete S3 topology returned to remote clients.

## Example Usage

```typescript
import { RemoteS3StorageBinding } from "@alienplatform/manager-api/models";

let value: RemoteS3StorageBinding = {
  bucketName: "<value>",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `bucketName`                                       | *string*                                           | :heavy_check_mark:                                 | S3 bucket name authorized by the credential lease. |