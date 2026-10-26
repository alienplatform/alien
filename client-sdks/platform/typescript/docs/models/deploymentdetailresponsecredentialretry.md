# DeploymentDetailResponseCredentialRetry

A credential outage's durable retry window, scoped to its deployment phase.

## Example Usage

```typescript
import { DeploymentDetailResponseCredentialRetry } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseCredentialRetry = {
  startedAt: new Date("2024-08-07T21:34:37.462Z"),
  status: "waiting-for-secrets",
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `startedAt`                                                                                                        | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                      | :heavy_check_mark:                                                                                                 | First credential failure in this phase; subsequent attempts keep this timestamp.                                   |
| `status`                                                                                                           | [models.DeploymentDetailResponseCredentialRetryStatus](../models/deploymentdetailresponsecredentialretrystatus.md) | :heavy_check_mark:                                                                                                 | Changing phase starts a new window instead of carrying a previous outage forward.                                  |