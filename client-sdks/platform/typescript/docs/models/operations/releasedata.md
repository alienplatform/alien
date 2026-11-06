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
      upToDate: 84741,
      waiting: 236888,
      unknown: 787885,
      failed: 433203,
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
| `rollout`                                                                                                                                                      | [operations.GetRemoteOperatorProjectSummaryRollout](../../models/operations/getremoteoperatorprojectsummaryrollout.md)[]                                       | :heavy_check_mark:                                                                                                                                             | Desired releases, most deployments first, with how many Remote Operator deployments run each one, still need the change, report no release, or failed after it |