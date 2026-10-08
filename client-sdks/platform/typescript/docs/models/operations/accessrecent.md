# AccessRecent

## Example Usage

```typescript
import { AccessRecent } from "@alienplatform/platform-api/models/operations";

let value: AccessRecent = {
  id: "<id>",
  deploymentId: "<id>",
  title: "<value>",
  status: "customer-approved",
  approvedUntil: new Date("2025-08-30T10:56:06.187Z"),
  updatedAt: new Date("2024-12-18T12:20:50.308Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `deploymentId`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `title`                                                                                       | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `status`                                                                                      | [operations.RecentStatus](../../models/operations/recentstatus.md)                            | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `approvedUntil`                                                                               | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `updatedAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |