# GetRemoteOperatorProjectSummaryRelease

## Example Usage

```typescript
import { GetRemoteOperatorProjectSummaryRelease } from "@alienplatform/platform-api/models/operations";

let value: GetRemoteOperatorProjectSummaryRelease = {
  status: "error",
  freshness: "unknown",
  sourceUpdatedAt: new Date("2025-11-26T21:40:24.869Z"),
  error: null,
  data: {
    reported: [
      {
        releaseId: "<id>",
        version: "<value>",
        createdAt: new Date("2026-02-12T11:27:57.156Z"),
        deployments: 179604,
        latest: true,
      },
    ],
    older: 344847,
    unknown: 288932,
    rollout: [
      {
        releaseId: "<id>",
        version: "<value>",
        upToDate: 786693,
        waiting: 96641,
        unknown: 669432,
        failed: 401240,
      },
    ],
  },
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `status`                                                                                      | [operations.ReleaseStatus](../../models/operations/releasestatus.md)                          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `freshness`                                                                                   | [operations.ReleaseFreshness](../../models/operations/releasefreshness.md)                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `sourceUpdatedAt`                                                                             | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `error`                                                                                       | [operations.ReleaseError](../../models/operations/releaseerror.md)                            | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `data`                                                                                        | [operations.ReleaseData](../../models/operations/releasedata.md)                              | :heavy_check_mark:                                                                            | N/A                                                                                           |