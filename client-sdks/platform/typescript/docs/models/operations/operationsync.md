# OperationSync

## Example Usage

```typescript
import { OperationSync } from "@alienplatform/platform-api/models/operations";

let value: OperationSync = {
  status: "syncing",
  targetBundleHash: "<value>",
  observedBundleHash: null,
  targetSetAt: new Date("2024-06-14T05:41:18.403Z"),
  observedAt: new Date("2026-07-28T22:46:13.848Z"),
  error: "<value>",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `status`                                                                                      | [operations.OperationSyncStatus](../../models/operations/operationsyncstatus.md)              | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `targetBundleHash`                                                                            | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `observedBundleHash`                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `targetSetAt`                                                                                 | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `observedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `error`                                                                                       | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |