# ProjectHealth

## Example Usage

```typescript
import { ProjectHealth } from "@alienplatform/platform-api/models";

let value: ProjectHealth = {
  total: 5922.65,
  running: 2507.2,
  reporting: 4662.54,
  attention: 5472.27,
  settingUp: 8379.94,
  strip: [],
  connectedGroups: 3400.35,
  production: {
    releaseId: "<id>",
    deploymentCount: 1317.86,
    currentReleaseCount: 3818.89,
    pinnedCount: 5352.16,
    updatingCount: 4255.8,
    failedCount: 9872.27,
    changedAt: new Date("2025-02-13T09:08:26.396Z"),
  },
  lastActivityAt: new Date("2025-02-14T06:19:16.987Z"),
};
```

## Fields

| Field                                                                                                   | Type                                                                                                    | Required                                                                                                | Description                                                                                             |
| ------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| `total`                                                                                                 | *number*                                                                                                | :heavy_check_mark:                                                                                      | Application deployments; gateway connections are not counted                                            |
| `running`                                                                                               | *number*                                                                                                | :heavy_check_mark:                                                                                      | Running with nothing to do, including those queued for a rollout                                        |
| `reporting`                                                                                             | *number*                                                                                                | :heavy_check_mark:                                                                                      | Running deployments that heartbeated in the last 5 minutes                                              |
| `attention`                                                                                             | *number*                                                                                                | :heavy_check_mark:                                                                                      | Deployments the deployment list shows under 'Needs attention'                                           |
| `settingUp`                                                                                             | *number*                                                                                                | :heavy_check_mark:                                                                                      | Deployments still being set up for the first time                                                       |
| `strip`                                                                                                 | [models.DeploymentMapState](../models/deploymentmapstate.md)[]                                          | :heavy_check_mark:                                                                                      | Up to 60 deployments' map states, deployments needing attention first                                   |
| `connectedGroups`                                                                                       | *number*                                                                                                | :heavy_check_mark:                                                                                      | Deployment groups with a running connection to the project's capabilities (non-application deployments) |
| `production`                                                                                            | [models.Production](../models/production.md)                                                            | :heavy_check_mark:                                                                                      | N/A                                                                                                     |
| `lastActivityAt`                                                                                        | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)           | :heavy_check_mark:                                                                                      | Newest release, production move, or new deployment                                                      |