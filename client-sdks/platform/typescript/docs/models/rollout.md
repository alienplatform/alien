# Rollout

Rollout stats, included when ?include=rollout is used

## Example Usage

```typescript
import { Rollout } from "@alienplatform/platform-api/models";

let value: Rollout = {
  updatedCount: 786693,
  pendingCount: 96641,
  avgDurationMs: 4012.4,
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `updatedCount`                                                                           | *number*                                                                                 | :heavy_check_mark:                                                                       | Deployments that finished updating to this release (excludes initial provisions)         |
| `pendingCount`                                                                           | *number*                                                                                 | :heavy_check_mark:                                                                       | Deployments currently targeting this release but not yet running it                      |
| `avgDurationMs`                                                                          | *number*                                                                                 | :heavy_check_mark:                                                                       | Average time from release creation until a deployment finished updating, in milliseconds |
