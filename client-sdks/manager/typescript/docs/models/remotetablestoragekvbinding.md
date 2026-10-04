# RemoteTableStorageKvBinding

Concrete Azure Table Storage KV topology returned to remote clients.

## Example Usage

```typescript
import { RemoteTableStorageKvBinding } from "@alienplatform/manager-api/models";

let value: RemoteTableStorageKvBinding = {
  accountName: "<value>",
  resourceGroupName: "<value>",
  tableName: "<value>",
};
```

## Fields

| Field                                     | Type                                      | Required                                  | Description                               |
| ----------------------------------------- | ----------------------------------------- | ----------------------------------------- | ----------------------------------------- |
| `accountName`                             | *string*                                  | :heavy_check_mark:                        | N/A                                       |
| `resourceGroupName`                       | *string*                                  | :heavy_check_mark:                        | N/A                                       |
| `tableName`                               | *string*                                  | :heavy_check_mark:                        | Table authorized by the credential lease. |