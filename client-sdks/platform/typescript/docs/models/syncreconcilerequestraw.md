# SyncReconcileRequestRaw

## Example Usage

```typescript
import { SyncReconcileRequestRaw } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestRaw = {
  body: "<value>",
  collectedAt: new Date("2025-08-06T05:07:31.209Z"),
  format: "text",
  source: "<value>",
  truncated: true,
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `body`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `collectedAt`                                                                                 | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `format`                                                                                      | [models.SyncReconcileRequestFormat](../models/syncreconcilerequestformat.md)                  | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `source`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `truncated`                                                                                   | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |