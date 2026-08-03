# RemoteGcsStorageBinding

Concrete Google Cloud Storage topology returned to remote clients.

## Example Usage

```typescript
import { RemoteGcsStorageBinding } from "@alienplatform/manager-api/models";

let value: RemoteGcsStorageBinding = {
  bucketName: "<value>",
};
```

## Fields

| Field                                               | Type                                                | Required                                            | Description                                         |
| --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- |
| `bucketName`                                        | *string*                                            | :heavy_check_mark:                                  | GCS bucket name authorized by the credential lease. |