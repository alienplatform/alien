# QueueAccessRequestRevokedBy

## Example Usage

```typescript
import { QueueAccessRequestRevokedBy } from "@alienplatform/platform-api/models/operations";

let value: QueueAccessRequestRevokedBy = {
  actorKind: "serviceAccount",
  actorId: "<id>",
  at: "<value>",
  reason: "<value>",
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `actorKind`                                                                                      | [operations.QueueAccessRequestActorKind](../../models/operations/queueaccessrequestactorkind.md) | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `actorId`                                                                                        | *string*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `at`                                                                                             | *string*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `reason`                                                                                         | *string*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |