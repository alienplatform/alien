# SyncAcquireRequest

Request to acquire deployments for processing

## Example Usage

```typescript
import { SyncAcquireRequest } from "@aliendotdev/platform-api/models";

let value: SyncAcquireRequest = {
  managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
  session: "<value>",
  deploymentIds: [
    "ag_pnj2da55wi5sxbdcav9t273je",
  ],
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                | Example                                                                                    |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `managerId`                                                                                | *string*                                                                                   | :heavy_minus_sign:                                                                         | N/A                                                                                        | mgr_enxscjrqiiu2lrc672hwwuc5                                                               |
| `session`                                                                                  | *string*                                                                                   | :heavy_check_mark:                                                                         | Unique session identifier for lock tracking                                                |                                                                                            |
| `deploymentIds`                                                                            | *string*[]                                                                                 | :heavy_minus_sign:                                                                         | Specific deployment IDs to lock (for Pull model sync)                                      |                                                                                            |
| `statuses`                                                                                 | [models.SyncAcquireRequestStatus](../models/syncacquirerequeststatus.md)[]                 | :heavy_minus_sign:                                                                         | Filter by deployment statuses (default: all deployment statuses)                           |                                                                                            |
| `platforms`                                                                                | [models.SyncAcquireRequestPlatform](../models/syncacquirerequestplatform.md)[]             | :heavy_minus_sign:                                                                         | Filter by platforms (default: all platforms the Manager supports)                          |                                                                                            |
| `deploymentModel`                                                                          | [models.SyncAcquireRequestDeploymentModel](../models/syncacquirerequestdeploymentmodel.md) | :heavy_minus_sign:                                                                         | Filter by deployment model from stackSettings.deploymentModel (Manager should use 'push')  |                                                                                            |
| `limit`                                                                                    | *number*                                                                                   | :heavy_minus_sign:                                                                         | Maximum number of deployments to acquire (default: 10)                                     |                                                                                            |