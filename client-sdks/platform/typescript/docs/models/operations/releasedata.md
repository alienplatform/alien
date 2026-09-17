# ReleaseData

## Example Usage

```typescript
import { ReleaseData } from "@alienplatform/platform-api/models/operations";

let value: ReleaseData = {
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
};
```

## Fields

| Field                                                                                                   | Type                                                                                                    | Required                                                                                                | Description                                                                                             |
| ------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| `active`                                                                                                | [operations.Active](../../models/operations/active.md)                                                  | :heavy_check_mark:                                                                                      | N/A                                                                                                     |
| `rollout`                                                                                               | [models.ReleaseRolloutStateCounts](../../models/releaserolloutstatecounts.md)                           | :heavy_check_mark:                                                                                      | Deployment counts per rollout state across all deployments the filters allow (ignores the state filter) |