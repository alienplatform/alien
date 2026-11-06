# CloudAccount

Subaccount summary, only when the caller can read the subaccount.

## Example Usage

```typescript
import { CloudAccount } from "@alienplatform/platform-api/models";

let value: CloudAccount = {
  id: "d5a9e5ca-3c69-4888-a1c6-d41b3db169b2",
  name: "<value>",
  providerId: "<id>",
  status: "Creating",
  reason: "<value>",
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `id`                                                         | *string*                                                     | :heavy_check_mark:                                           | N/A                                                          |
| `name`                                                       | *string*                                                     | :heavy_check_mark:                                           | N/A                                                          |
| `providerId`                                                 | *string*                                                     | :heavy_check_mark:                                           | N/A                                                          |
| `status`                                                     | [models.CloudAccountStatus](../models/cloudaccountstatus.md) | :heavy_check_mark:                                           | N/A                                                          |
| `reason`                                                     | *string*                                                     | :heavy_check_mark:                                           | N/A                                                          |