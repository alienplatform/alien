# ListBillingAuditLogResponse

Audit-log rows newest-first.

## Example Usage

```typescript
import { ListBillingAuditLogResponse } from "@alienplatform/platform-api/models/operations";

let value: ListBillingAuditLogResponse = {
  items: [
    {
      id: "<id>",
      workspaceId: "<id>",
      action: "<value>",
      createdAt: "1717600910817",
    },
  ],
  nextCursor: "<value>",
};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `items`                                                           | [models.BillingAuditLogRow](../../models/billingauditlogrow.md)[] | :heavy_check_mark:                                                | N/A                                                               |
| `nextCursor`                                                      | *string*                                                          | :heavy_check_mark:                                                | N/A                                                               |