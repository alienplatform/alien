# ResolvePendingChangeRequest

## Example Usage

```typescript
import { ResolvePendingChangeRequest } from "@alienplatform/manager-api/models/operations";

let value: ResolvePendingChangeRequest = {
  id: "<id>",
  resolvePendingChangeRequest: {
    status: "rejected",
  },
};
```

## Fields

| Field                                                                             | Type                                                                              | Required                                                                          | Description                                                                       |
| --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| `id`                                                                              | *string*                                                                          | :heavy_check_mark:                                                                | Pending change id                                                                 |
| `resolvePendingChangeRequest`                                                     | [models.ResolvePendingChangeRequest](../../models/resolvependingchangerequest.md) | :heavy_check_mark:                                                                | N/A                                                                               |
