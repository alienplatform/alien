# DeploymentStats

## Example Usage

```typescript
import { DeploymentStats } from "@alienplatform/platform-api/models";

let value: DeploymentStats = {
  total: 3462.34,
  byStatus: {
    "key": 6293.18,
    "key1": 6338.62,
    "key2": 7849.55,
  },
  byPlatform: {
    "key": 9884.37,
    "key1": 2467.03,
    "key2": 2588.97,
  },
  byCurrentRelease: {
    "key": 8084.64,
    "key1": 7403.75,
  },
  byPinnedRelease: {
    "key": 2517.74,
    "key1": 3235.28,
  },
};
```

## Fields

| Field                                                                                                                                 | Type                                                                                                                                  | Required                                                                                                                              | Description                                                                                                                           |
| ------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| `total`                                                                                                                               | *number*                                                                                                                              | :heavy_check_mark:                                                                                                                    | Total number of deployments matching filters                                                                                          |
| `byStatus`                                                                                                                            | Record<string, *number*>                                                                                                              | :heavy_check_mark:                                                                                                                    | Count of deployments by status (only includes statuses with non-zero counts)                                                          |
| `byPlatform`                                                                                                                          | Record<string, *number*>                                                                                                              | :heavy_check_mark:                                                                                                                    | Count of deployments by platform (only includes platforms with non-zero counts)                                                       |
| `byCurrentRelease`                                                                                                                    | Record<string, *number*>                                                                                                              | :heavy_check_mark:                                                                                                                    | Count of deployments by currentReleaseId. The empty string key represents deployments with no current release (initial provisioning). |
| `byPinnedRelease`                                                                                                                     | Record<string, *number*>                                                                                                              | :heavy_check_mark:                                                                                                                    | Count of deployments by pinnedReleaseId among deployments that are pinned. Excludes unpinned deployments.                             |