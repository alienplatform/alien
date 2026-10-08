# DenyAccessRequestRevokedBy

## Example Usage

```typescript
import { DenyAccessRequestRevokedBy } from "@alienplatform/platform-api/models/operations";

let value: DenyAccessRequestRevokedBy = {
  actorKind: "serviceAccount",
  actorId: "<id>",
  at: "<value>",
  reason: "<value>",
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `actorKind`                                                                                    | [operations.DenyAccessRequestActorKind](../../models/operations/denyaccessrequestactorkind.md) | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `actorId`                                                                                      | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `at`                                                                                           | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `reason`                                                                                       | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |
