# SyncListResponseCredentialRetry

A credential outage's durable retry window, scoped to its deployment phase.

## Example Usage

```typescript
import { SyncListResponseCredentialRetry } from "@alienplatform/platform-api/models";

let value: SyncListResponseCredentialRetry = {
  startedAt: new Date("2025-04-03T15:49:06.584Z"),
  status: "waiting-for-secrets",
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `startedAt`                                                                                        | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)      | :heavy_check_mark:                                                                                 | First credential failure in this phase; subsequent attempts keep this timestamp.                   |
| `status`                                                                                           | [models.SyncListResponseCredentialRetryStatus](../models/synclistresponsecredentialretrystatus.md) | :heavy_check_mark:                                                                                 | Changing phase starts a new window instead of carrying a previous outage forward.                  |