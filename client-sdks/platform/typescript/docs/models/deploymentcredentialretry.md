# DeploymentCredentialRetry

A credential outage's durable retry window, scoped to its deployment phase.

## Example Usage

```typescript
import { DeploymentCredentialRetry } from "@alienplatform/platform-api/models";

let value: DeploymentCredentialRetry = {
  startedAt: new Date("2025-04-10T17:19:26.230Z"),
  status: "teardown-failed",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `startedAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | First credential failure in this phase; subsequent attempts keep this timestamp.              |
| `status`                                                                                      | [models.DeploymentCredentialRetryStatus](../models/deploymentcredentialretrystatus.md)        | :heavy_check_mark:                                                                            | Changing phase starts a new window instead of carrying a previous outage forward.             |