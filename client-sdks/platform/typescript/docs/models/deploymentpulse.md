# DeploymentPulse

## Example Usage

```typescript
import { DeploymentPulse } from "@alienplatform/platform-api/models";

let value: DeploymentPulse = {
  total: 5047.02,
  expected: 1296.96,
  reportingNow: 4726.39,
  history: {
    status: "unavailable",
    reason: "not-configured",
  },
};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `total`                                                                                                                          | *number*                                                                                                                         | :heavy_check_mark:                                                                                                               | Application deployments of the project                                                                                           |
| `expected`                                                                                                                       | *number*                                                                                                                         | :heavy_check_mark:                                                                                                               | Of those, the ones expected to heartbeat: serving a release (running, updating, or after a failed update), online, heartbeats on |
| `reportingNow`                                                                                                                   | *number*                                                                                                                         | :heavy_check_mark:                                                                                                               | Of the expected ones, how many heartbeated in the last 5 minutes                                                                 |
| `history`                                                                                                                        | *models.History*                                                                                                                 | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |