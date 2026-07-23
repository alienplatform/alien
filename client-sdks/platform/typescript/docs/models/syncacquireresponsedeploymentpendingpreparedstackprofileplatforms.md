# SyncAcquireResponseDeploymentPendingPreparedStackProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPendingPreparedStackProfilePlatforms } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPendingPreparedStackProfilePlatforms =
  {};
```

## Fields

| Field                                                                                                                                                | Type                                                                                                                                                 | Required                                                                                                                                             | Description                                                                                                                                          |
| ---------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                                                | [models.SyncAcquireResponseDeploymentPendingPreparedStackProfileAw](../models/syncacquireresponsedeploymentpendingpreparedstackprofileaw.md)[]       | :heavy_minus_sign:                                                                                                                                   | AWS permission configurations                                                                                                                        |
| `azure`                                                                                                                                              | [models.SyncAcquireResponseDeploymentPendingPreparedStackProfileAzure](../models/syncacquireresponsedeploymentpendingpreparedstackprofileazure.md)[] | :heavy_minus_sign:                                                                                                                                   | Azure permission configurations                                                                                                                      |
| `gcp`                                                                                                                                                | [models.SyncAcquireResponseDeploymentPendingPreparedStackProfileGcp](../models/syncacquireresponsedeploymentpendingpreparedstackprofilegcp.md)[]     | :heavy_minus_sign:                                                                                                                                   | GCP permission configurations                                                                                                                        |
