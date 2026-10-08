# ApproveAccessRequestRevokedBy

## Example Usage

```typescript
import { ApproveAccessRequestRevokedBy } from "@alienplatform/platform-api/models/operations";

let value: ApproveAccessRequestRevokedBy = {
  actorKind: "serviceAccount",
  actorId: "<id>",
  at: "<value>",
  reason: "<value>",
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `actorKind`                                                                                          | [operations.ApproveAccessRequestActorKind](../../models/operations/approveaccessrequestactorkind.md) | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `actorId`                                                                                            | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `at`                                                                                                 | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `reason`                                                                                             | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |