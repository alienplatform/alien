# SyncAcquireResponseDeploymentPendingPreparedStackOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPendingPreparedStackOverridePlatforms } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPendingPreparedStackOverridePlatforms =
  {};
```

## Fields

| Field                                                                                                                                                  | Type                                                                                                                                                   | Required                                                                                                                                               | Description                                                                                                                                            |
| ------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                                                                  | [models.SyncAcquireResponseDeploymentPendingPreparedStackOverrideAw](../models/syncacquireresponsedeploymentpendingpreparedstackoverrideaw.md)[]       | :heavy_minus_sign:                                                                                                                                     | AWS permission configurations                                                                                                                          |
| `azure`                                                                                                                                                | [models.SyncAcquireResponseDeploymentPendingPreparedStackOverrideAzure](../models/syncacquireresponsedeploymentpendingpreparedstackoverrideazure.md)[] | :heavy_minus_sign:                                                                                                                                     | Azure permission configurations                                                                                                                        |
| `gcp`                                                                                                                                                  | [models.SyncAcquireResponseDeploymentPendingPreparedStackOverrideGcp](../models/syncacquireresponsedeploymentpendingpreparedstackoverridegcp.md)[]     | :heavy_minus_sign:                                                                                                                                     | GCP permission configurations                                                                                                                          |
