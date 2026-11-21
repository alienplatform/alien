# ListPendingChangesRequest

## Example Usage

```typescript
import { ListPendingChangesRequest } from "@alienplatform/platform-api/models/operations";

let value: ListPendingChangesRequest = {
  idOrName: "<value>",
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `idOrName`                                                                                 | *string*                                                                                   | :heavy_check_mark:                                                                         | Project ID or name.                                                                        |
| `status`                                                                                   | [operations.ListPendingChangesStatus](../../models/operations/listpendingchangesstatus.md) | :heavy_minus_sign:                                                                         | Only changes with this status. Defaults to pending.                                        |