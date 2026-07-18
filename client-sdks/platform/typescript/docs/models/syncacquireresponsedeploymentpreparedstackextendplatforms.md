# SyncAcquireResponseDeploymentPreparedStackExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPreparedStackExtendPlatforms } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPreparedStackExtendPlatforms = {};
```

## Fields

| Field                                                                                                                                | Type                                                                                                                                 | Required                                                                                                                             | Description                                                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                                                | [models.SyncAcquireResponseDeploymentPreparedStackExtendAw](../models/syncacquireresponsedeploymentpreparedstackextendaw.md)[]       | :heavy_minus_sign:                                                                                                                   | AWS permission configurations                                                                                                        |
| `azure`                                                                                                                              | [models.SyncAcquireResponseDeploymentPreparedStackExtendAzure](../models/syncacquireresponsedeploymentpreparedstackextendazure.md)[] | :heavy_minus_sign:                                                                                                                   | Azure permission configurations                                                                                                      |
| `gcp`                                                                                                                                | [models.SyncAcquireResponseDeploymentPreparedStackExtendGcp](../models/syncacquireresponsedeploymentpreparedstackextendgcp.md)[]     | :heavy_minus_sign:                                                                                                                   | GCP permission configurations                                                                                                        |