# DeploymentPreparedStackOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentPreparedStackOverridePlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentPreparedStackOverridePlatforms = {};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `aws`                                                                                              | [models.DeploymentPreparedStackOverrideAw](../models/deploymentpreparedstackoverrideaw.md)[]       | :heavy_minus_sign:                                                                                 | AWS permission configurations                                                                      |
| `azure`                                                                                            | [models.DeploymentPreparedStackOverrideAzure](../models/deploymentpreparedstackoverrideazure.md)[] | :heavy_minus_sign:                                                                                 | Azure permission configurations                                                                    |
| `gcp`                                                                                              | [models.DeploymentPreparedStackOverrideGcp](../models/deploymentpreparedstackoverridegcp.md)[]     | :heavy_minus_sign:                                                                                 | GCP permission configurations                                                                      |
