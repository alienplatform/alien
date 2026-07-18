# SyncAcquireResponseDeploymentPreparedStackOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPreparedStackOverridePlatforms } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPreparedStackOverridePlatforms = {};
```

## Fields

| Field                                                                                                                                    | Type                                                                                                                                     | Required                                                                                                                                 | Description                                                                                                                              |
| ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                                    | [models.SyncAcquireResponseDeploymentPreparedStackOverrideAw](../models/syncacquireresponsedeploymentpreparedstackoverrideaw.md)[]       | :heavy_minus_sign:                                                                                                                       | AWS permission configurations                                                                                                            |
| `azure`                                                                                                                                  | [models.SyncAcquireResponseDeploymentPreparedStackOverrideAzure](../models/syncacquireresponsedeploymentpreparedstackoverrideazure.md)[] | :heavy_minus_sign:                                                                                                                       | Azure permission configurations                                                                                                          |
| `gcp`                                                                                                                                    | [models.SyncAcquireResponseDeploymentPreparedStackOverrideGcp](../models/syncacquireresponsedeploymentpreparedstackoverridegcp.md)[]     | :heavy_minus_sign:                                                                                                                       | GCP permission configurations                                                                                                            |