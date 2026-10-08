# GetAccessRequestRevokedBy

## Example Usage

```typescript
import { GetAccessRequestRevokedBy } from "@alienplatform/platform-api/models/operations";

let value: GetAccessRequestRevokedBy = {
  actorKind: "user",
  actorId: "<id>",
  at: "<value>",
  reason: "<value>",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `actorKind`                                                                                  | [operations.GetAccessRequestActorKind](../../models/operations/getaccessrequestactorkind.md) | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `actorId`                                                                                    | *string*                                                                                     | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `at`                                                                                         | *string*                                                                                     | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `reason`                                                                                     | *string*                                                                                     | :heavy_check_mark:                                                                           | N/A                                                                                          |