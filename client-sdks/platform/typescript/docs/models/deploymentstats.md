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
};
```

## Fields

| Field                                                                           | Type                                                                            | Required                                                                        | Description                                                                     |
| ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| `total`                                                                         | *number*                                                                        | :heavy_check_mark:                                                              | Total number of deployments matching filters                                    |
| `byStatus`                                                                      | Record<string, *number*>                                                        | :heavy_check_mark:                                                              | Count of deployments by status (only includes statuses with non-zero counts)    |
| `byPlatform`                                                                    | Record<string, *number*>                                                        | :heavy_check_mark:                                                              | Count of deployments by platform (only includes platforms with non-zero counts) |