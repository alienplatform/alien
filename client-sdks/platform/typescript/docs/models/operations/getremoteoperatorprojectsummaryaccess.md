# GetRemoteOperatorProjectSummaryAccess

## Example Usage

```typescript
import { GetRemoteOperatorProjectSummaryAccess } from "@alienplatform/platform-api/models/operations";

let value: GetRemoteOperatorProjectSummaryAccess = {
  status: "unavailable",
  freshness: "stale",
  sourceUpdatedAt: new Date("2026-04-27T15:04:56.763Z"),
  error: {
    code: "<value>",
    message: "<value>",
  },
  data: {
    pending: 288049,
    active: 604277,
    recent: [],
  },
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `status`                                                                                      | [operations.AccessStatus](../../models/operations/accessstatus.md)                            | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `freshness`                                                                                   | [operations.AccessFreshness](../../models/operations/accessfreshness.md)                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `sourceUpdatedAt`                                                                             | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `error`                                                                                       | [operations.AccessError](../../models/operations/accesserror.md)                              | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `data`                                                                                        | [operations.AccessData](../../models/operations/accessdata.md)                                | :heavy_check_mark:                                                                            | N/A                                                                                           |