# SyncAcquireResponse

Acquired deployments and failures

## Example Usage

```typescript
import { SyncAcquireResponse } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponse = {
  deployments: [],
  failures: [],
  notAcquired: [
    {
      deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
      reason: "limitReached",
    },
  ],
  leaseExpiresAt: new Date("2024-08-03T10:13:46.884Z"),
};
```

## Fields

| Field                                                                                                                                             | Type                                                                                                                                              | Required                                                                                                                                          | Description                                                                                                                                       |
| ------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| `deployments`                                                                                                                                     | [models.SyncAcquireResponseDeployment](../models/syncacquireresponsedeployment.md)[]                                                              | :heavy_check_mark:                                                                                                                                | List of acquired deployments with deployment context                                                                                              |
| `failures`                                                                                                                                        | [models.Failure](../models/failure.md)[]                                                                                                          | :heavy_check_mark:                                                                                                                                | List of deployments that failed during context building (locks already released)                                                                  |
| `notAcquired`                                                                                                                                     | [models.UnacquiredDeployment](../models/unacquireddeployment.md)[]                                                                                | :heavy_minus_sign:                                                                                                                                | Bounded reasons for explicitly requested deployments that were not acquired. Empty for discovery batches.                                         |
| `leaseExpiresAt`                                                                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                                                     | :heavy_check_mark:                                                                                                                                | When the provisional leases on the returned deployments lapse. Confirm them with sync/renew before starting work. Null when nothing was acquired. |