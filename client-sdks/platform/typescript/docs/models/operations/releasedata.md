# ReleaseData

## Example Usage

```typescript
import { ReleaseData } from "@alienplatform/platform-api/models/operations";

let value: ReleaseData = {
  reported: [
    {
      releaseId: "<id>",
      version: "<value>",
      createdAt: new Date("2026-02-12T11:27:57.156Z"),
      deployments: 179604,
      latest: true,
    },
  ],
  older: 504443,
  unknown: 107393,
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
};
```

## Fields

| Field                                                                                                                                                          | Type                                                                                                                                                           | Required                                                                                                                                                       | Description                                                                                                                                                    |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `reported`                                                                                                                                                     | [operations.ReleaseReported](../../models/operations/releasereported.md)[]                                                                                     | :heavy_check_mark:                                                                                                                                             | Application releases that Remote Operator deployments report, newest first. The newest reported release is latest.                                             |
| `older`                                                                                                                                                        | *number*                                                                                                                                                       | :heavy_check_mark:                                                                                                                                             | Deployments without a desired release that report a release older than the latest reported release                                                             |
| `unknown`                                                                                                                                                      | *number*                                                                                                                                                       | :heavy_check_mark:                                                                                                                                             | Deployments that report no application release                                                                                                                 |
| `rollout`                                                                                                                                                      | [operations.Rollout](../../models/operations/rollout.md)[]                                                                                                     | :heavy_check_mark:                                                                                                                                             | Desired releases, most deployments first, with how many Remote Operator deployments run each one, still need the change, report no release, or failed after it |