# DeploymentStateCredentialRetry

A credential outage's durable retry window, scoped to its deployment phase.

## Example Usage

```typescript
import { DeploymentStateCredentialRetry } from "@alienplatform/platform-api/models";

let value: DeploymentStateCredentialRetry = {
  startedAt: new Date("2024-01-03T18:40:43.527Z"),
  status: "deleting",
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `startedAt`                                                                                      | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)    | :heavy_check_mark:                                                                               | First credential failure in this phase; subsequent attempts keep this timestamp.                 |
| `status`                                                                                         | [models.DeploymentStateCredentialRetryStatus](../models/deploymentstatecredentialretrystatus.md) | :heavy_check_mark:                                                                               | Changing phase starts a new window instead of carrying a previous outage forward.                |