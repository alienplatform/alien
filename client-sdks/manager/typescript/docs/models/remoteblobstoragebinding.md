# RemoteBlobStorageBinding

Concrete Azure Blob Storage topology returned to remote clients.

## Example Usage

```typescript
import { RemoteBlobStorageBinding } from "@alienplatform/manager-api/models";

let value: RemoteBlobStorageBinding = {
  accountName: "<value>",
  containerName: "<value>",
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `accountName`                                        | *string*                                             | :heavy_check_mark:                                   | Storage account containing the authorized container. |
| `containerName`                                      | *string*                                             | :heavy_check_mark:                                   | Blob container authorized by the credential lease.   |