# SyncAcquireResponseDeployment

## Example Usage

```typescript
import { SyncAcquireResponseDeployment } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseDeployment = {
  deploymentId: "ag_pnj2da55wi5sxbdcav9t273je",
  projectId: "<id>",
  current: {
    platform: "aws",
    status: "updating",
  },
  config: {
    environmentVariables: {
      createdAt: "1728576721265",
      hash: "<value>",
      variables: [],
    },
  },
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  | Example                                                                      |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `deploymentId`                                                               | *string*                                                                     | :heavy_check_mark:                                                           | ID of the acquired deployment                                                | ag_pnj2da55wi5sxbdcav9t273je                                                 |
| `projectId`                                                                  | *string*                                                                     | :heavy_check_mark:                                                           | Project ID the agent belongs to                                              |                                                                              |
| `current`                                                                    | [models.SyncAcquireResponseCurrent](../models/syncacquireresponsecurrent.md) | :heavy_check_mark:                                                           | Current deployment state (includes releases)                                 |                                                                              |
| `config`                                                                     | [models.SyncAcquireResponseConfig](../models/syncacquireresponseconfig.md)   | :heavy_check_mark:                                                           | Deployment configuration                                                     |                                                                              |