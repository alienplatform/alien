# ReleaseDeploymentsResponse

## Example Usage

```typescript
import { ReleaseDeploymentsResponse } from "@alienplatform/platform-api/models";

let value: ReleaseDeploymentsResponse = {
  items: [],
  nextCursor: "<value>",
  stateCounts: {
    updated: 256940,
    updating: 670112,
    failed: 812790,
    pending: 710404,
    pinnedOther: 896373,
    superseded: 835410,
    onOther: 213666,
  },
};
```

## Fields

| Field                                                                                                   | Type                                                                                                    | Required                                                                                                | Description                                                                                             |
| ------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| `items`                                                                                                 | [models.ReleaseDeploymentItem](../models/releasedeploymentitem.md)[]                                    | :heavy_check_mark:                                                                                      | Items in this page                                                                                      |
| `nextCursor`                                                                                            | *string*                                                                                                | :heavy_check_mark:                                                                                      | Cursor for the next page, null if last page                                                             |
| `stateCounts`                                                                                           | [models.ReleaseRolloutStateCounts](../models/releaserolloutstatecounts.md)                              | :heavy_check_mark:                                                                                      | Deployment counts per rollout state across all deployments the filters allow (ignores the state filter) |