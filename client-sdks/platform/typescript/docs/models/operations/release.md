# Release

## Example Usage

```typescript
import { Release } from "@alienplatform/platform-api/models/operations";

let value: Release = {
  status: "ready",
  freshness: "unknown",
  sourceUpdatedAt: null,
  error: {
    code: "<value>",
    message: "<value>",
  },
  data: {
    active: {
      id: "<id>",
      version: "<value>",
      createdAt: new Date("2026-02-12T11:27:57.156Z"),
    },
    rollout: {
      updated: 179604,
      updating: 403863,
      failed: 504443,
      pending: 107393,
      pinnedOther: 637480,
      superseded: 84741,
      onOther: 236888,
    },
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