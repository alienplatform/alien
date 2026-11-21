# CreateAccessRequestApprovedBy

How and when the customer gate was passed. Null until approved.

## Example Usage

```typescript
import { CreateAccessRequestApprovedBy } from "@alienplatform/platform-api/models/operations";

let value: CreateAccessRequestApprovedBy = {
  method: "<value>",
  actorId: "<id>",
  at: "<value>",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `method`                                                                                                 | *string*                                                                                                 | :heavy_check_mark:                                                                                       | `kubectl` for the in-cluster path, else the direct caller's method such as `slack`.                      |
| `actorId`                                                                                                | *string*                                                                                                 | :heavy_check_mark:                                                                                       | The approving user. Null for the kubectl path: the approver is only in the customer's cluster audit log. |
| `at`                                                                                                     | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |