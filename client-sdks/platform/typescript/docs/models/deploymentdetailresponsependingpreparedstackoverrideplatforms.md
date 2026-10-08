# DeploymentDetailResponsePendingPreparedStackOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentDetailResponsePendingPreparedStackOverridePlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePendingPreparedStackOverridePlatforms = {};
```

## Fields

| Field                                                                                                                                        | Type                                                                                                                                         | Required                                                                                                                                     | Description                                                                                                                                  |
| -------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                                        | [models.DeploymentDetailResponsePendingPreparedStackOverrideAw](../models/deploymentdetailresponsependingpreparedstackoverrideaw.md)[]       | :heavy_minus_sign:                                                                                                                           | AWS permission configurations                                                                                                                |
| `azure`                                                                                                                                      | [models.DeploymentDetailResponsePendingPreparedStackOverrideAzure](../models/deploymentdetailresponsependingpreparedstackoverrideazure.md)[] | :heavy_minus_sign:                                                                                                                           | Azure permission configurations                                                                                                              |
| `gcp`                                                                                                                                        | [models.DeploymentDetailResponsePendingPreparedStackOverrideGcp](../models/deploymentdetailresponsependingpreparedstackoverridegcp.md)[]     | :heavy_minus_sign:                                                                                                                           | GCP permission configurations                                                                                                                |
