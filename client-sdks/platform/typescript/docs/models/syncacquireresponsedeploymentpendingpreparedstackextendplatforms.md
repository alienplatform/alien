# SyncAcquireResponseDeploymentPendingPreparedStackExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPendingPreparedStackExtendPlatforms } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPendingPreparedStackExtendPlatforms =
  {};
```

## Fields

| Field                                                                                                                                              | Type                                                                                                                                               | Required                                                                                                                                           | Description                                                                                                                                        |
| -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                                              | [models.SyncAcquireResponseDeploymentPendingPreparedStackExtendAw](../models/syncacquireresponsedeploymentpendingpreparedstackextendaw.md)[]       | :heavy_minus_sign:                                                                                                                                 | AWS permission configurations                                                                                                                      |
| `azure`                                                                                                                                            | [models.SyncAcquireResponseDeploymentPendingPreparedStackExtendAzure](../models/syncacquireresponsedeploymentpendingpreparedstackextendazure.md)[] | :heavy_minus_sign:                                                                                                                                 | Azure permission configurations                                                                                                                    |
| `gcp`                                                                                                                                              | [models.SyncAcquireResponseDeploymentPendingPreparedStackExtendGcp](../models/syncacquireresponsedeploymentpendingpreparedstackextendgcp.md)[]     | :heavy_minus_sign:                                                                                                                                 | GCP permission configurations                                                                                                                      |
