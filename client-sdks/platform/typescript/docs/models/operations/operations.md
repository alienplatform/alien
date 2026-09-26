# Operations

## Example Usage

```typescript
import { Operations } from "@alienplatform/platform-api/models/operations";

let value: Operations = {
  status: "unavailable",
  freshness: "stale",
  sourceUpdatedAt: new Date("2026-11-25T05:03:30.101Z"),
  error: {
    code: "<value>",
    message: "<value>",
  },
  data: {
    selected: [],
    sync: {
      preparing: 460444,
      syncing: 482425,
      ready: 229664,
      stuck: 429061,
    },
    generatedPermissions: [],
  },
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `status`                                                                                      | [operations.OperationsStatus](../../models/operations/operationsstatus.md)                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `freshness`                                                                                   | [operations.OperationsFreshness](../../models/operations/operationsfreshness.md)              | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `sourceUpdatedAt`                                                                             | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `error`                                                                                       | [operations.OperationsError](../../models/operations/operationserror.md)                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `data`                                                                                        | [operations.OperationsData](../../models/operations/operationsdata.md)                        | :heavy_check_mark:                                                                            | N/A                                                                                           |