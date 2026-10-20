# PersistImportedDeploymentRequestCredentialRetry

A credential outage's durable retry window, scoped to its deployment phase.

## Example Usage

```typescript
import { PersistImportedDeploymentRequestCredentialRetry } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestCredentialRetry = {
  startedAt: new Date("2026-04-06T19:14:55.016Z"),
  status: "waiting-for-machines",
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `startedAt`                                                                                                                        | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                                      | :heavy_check_mark:                                                                                                                 | First credential failure in this phase; subsequent attempts keep this timestamp.                                                   |
| `status`                                                                                                                           | [models.PersistImportedDeploymentRequestCredentialRetryStatus](../models/persistimporteddeploymentrequestcredentialretrystatus.md) | :heavy_check_mark:                                                                                                                 | Changing phase starts a new window instead of carrying a previous outage forward.                                                  |