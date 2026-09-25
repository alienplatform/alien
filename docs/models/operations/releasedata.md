# ReleaseData

## Example Usage

```typescript
import { ReleaseData } from "@alienplatform/platform-api/models/operations";

let value: ReleaseData = {
  reported: [
    {
      releaseId: "<id>",
      version: "<value>",
      createdAt: new Date("2024-02-18T08:51:45.483Z"),
      deployments: 64715,
      latest: true,
    },
  ],
  older: 705728,
  unknown: 179604,
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `reported`                                                                                                         | [operations.Reported](../../models/operations/reported.md)[]                                                       | :heavy_check_mark:                                                                                                 | Application releases that Remote Operator deployments report, newest first. The newest reported release is latest. |
| `older`                                                                                                            | *number*                                                                                                           | :heavy_check_mark:                                                                                                 | Deployments that report a release older than the latest reported release                                           |
| `unknown`                                                                                                          | *number*                                                                                                           | :heavy_check_mark:                                                                                                 | Deployments that report no application release                                                                     |