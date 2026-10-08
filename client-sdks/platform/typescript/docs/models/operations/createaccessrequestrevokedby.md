# CreateAccessRequestRevokedBy

## Example Usage

```typescript
import { CreateAccessRequestRevokedBy } from "@alienplatform/platform-api/models/operations";

let value: CreateAccessRequestRevokedBy = {
  actorKind: "serviceAccount",
  actorId: "<id>",
  at: "<value>",
  reason: "<value>",
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `actorKind`                                                                                        | [operations.CreateAccessRequestActorKind](../../models/operations/createaccessrequestactorkind.md) | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `actorId`                                                                                          | *string*                                                                                           | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `at`                                                                                               | *string*                                                                                           | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `reason`                                                                                           | *string*                                                                                           | :heavy_check_mark:                                                                                 | N/A                                                                                                |
