# ListAccessRequestsRevokedBy

## Example Usage

```typescript
import { ListAccessRequestsRevokedBy } from "@alienplatform/platform-api/models/operations";

let value: ListAccessRequestsRevokedBy = {
  actorKind: "serviceAccount",
  actorId: "<id>",
  at: "<value>",
  reason: "<value>",
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `actorKind`                                                                                      | [operations.ListAccessRequestsActorKind](../../models/operations/listaccessrequestsactorkind.md) | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `actorId`                                                                                        | *string*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `at`                                                                                             | *string*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `reason`                                                                                         | *string*                                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |
