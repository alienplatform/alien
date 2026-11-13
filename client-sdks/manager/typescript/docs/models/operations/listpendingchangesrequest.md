# ListPendingChangesRequest

## Example Usage

```typescript
import { ListPendingChangesRequest } from "@alienplatform/manager-api/models/operations";

let value: ListPendingChangesRequest = {
  project: "<value>",
};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `project`                                                         | *string*                                                          | :heavy_check_mark:                                                | Project id                                                        |
| `status`                                                          | [models.PendingChangeStatus](../../models/pendingchangestatus.md) | :heavy_minus_sign:                                                | Only changes with this status.                                    |
