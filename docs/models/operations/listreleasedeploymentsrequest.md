# ListReleaseDeploymentsRequest

## Example Usage

```typescript
import { ListReleaseDeploymentsRequest } from "@alienplatform/platform-api/models/operations";

let value: ListReleaseDeploymentsRequest = {
  id: "rel_WbhQgksrawSKIpEN0NAssHX9",
};
```

## Fields

| Field                                                               | Type                                                                | Required                                                            | Description                                                         | Example                                                             |
| ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- |
| `id`                                                                | *string*                                                            | :heavy_check_mark:                                                  | Unique identifier for the release.                                  | rel_WbhQgksrawSKIpEN0NAssHX9                                        |
| `state`                                                             | [models.ReleaseRolloutState](../../models/releaserolloutstate.md)[] | :heavy_minus_sign:                                                  | Filter deployments by rollout state                                 |                                                                     |
| `deploymentGroup`                                                   | *string*                                                            | :heavy_minus_sign:                                                  | Filter by deployment group ID or name                               |                                                                     |
| `limit`                                                             | *number*                                                            | :heavy_minus_sign:                                                  | Maximum number of items to return per page                          |                                                                     |
| `cursor`                                                            | *string*                                                            | :heavy_minus_sign:                                                  | Cursor for pagination - omit for first page                         |                                                                     |