# SyncAcquireResponse

Acquired deployments and failures

## Example Usage

```typescript
import { SyncAcquireResponse } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponse = {
  deployments: [],
  failures: [],
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `deployments`                                                                        | [models.SyncAcquireResponseDeployment](../models/syncacquireresponsedeployment.md)[] | :heavy_check_mark:                                                                   | List of acquired deployments with deployment context                                 |
| `failures`                                                                           | [models.Failure](../models/failure.md)[]                                             | :heavy_check_mark:                                                                   | List of deployments that failed during context building (locks already released)     |