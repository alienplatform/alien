# SyncAcquireResponse

Acquired deployments and failures

## Example Usage

```typescript
import { SyncAcquireResponse } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponse = {
  deployments: [],
  failures: [],
  leaseExpiresAt: new Date("2025-06-11T10:38:09.056Z"),
};
```

## Fields

| Field                                                                                                                                             | Type                                                                                                                                              | Required                                                                                                                                          | Description                                                                                                                                       |
| ------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| `deployments`                                                                                                                                     | [models.SyncAcquireResponseDeployment](../models/syncacquireresponsedeployment.md)[]                                                              | :heavy_check_mark:                                                                                                                                | List of acquired deployments with deployment context                                                                                              |
| `failures`                                                                                                                                        | [models.Failure](../models/failure.md)[]                                                                                                          | :heavy_check_mark:                                                                                                                                | List of deployments that failed during context building (locks already released)                                                                  |
| `leaseExpiresAt`                                                                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                                                     | :heavy_check_mark:                                                                                                                                | When the provisional leases on the returned deployments lapse. Confirm them with sync/renew before starting work. Null when nothing was acquired. |
