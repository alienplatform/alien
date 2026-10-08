# DeploymentStatePendingPreparedStackOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentStatePendingPreparedStackOverridePlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentStatePendingPreparedStackOverridePlatforms = {};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                      | [models.DeploymentStatePendingPreparedStackOverrideAw](../models/deploymentstatependingpreparedstackoverrideaw.md)[]       | :heavy_minus_sign:                                                                                                         | AWS permission configurations                                                                                              |
| `azure`                                                                                                                    | [models.DeploymentStatePendingPreparedStackOverrideAzure](../models/deploymentstatependingpreparedstackoverrideazure.md)[] | :heavy_minus_sign:                                                                                                         | Azure permission configurations                                                                                            |
| `gcp`                                                                                                                      | [models.DeploymentStatePendingPreparedStackOverrideGcp](../models/deploymentstatependingpreparedstackoverridegcp.md)[]     | :heavy_minus_sign:                                                                                                         | GCP permission configurations                                                                                              |