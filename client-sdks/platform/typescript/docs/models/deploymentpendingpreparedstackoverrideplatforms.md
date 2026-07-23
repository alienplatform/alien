# DeploymentPendingPreparedStackOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentPendingPreparedStackOverridePlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentPendingPreparedStackOverridePlatforms = {};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                            | [models.DeploymentPendingPreparedStackOverrideAw](../models/deploymentpendingpreparedstackoverrideaw.md)[]       | :heavy_minus_sign:                                                                                               | AWS permission configurations                                                                                    |
| `azure`                                                                                                          | [models.DeploymentPendingPreparedStackOverrideAzure](../models/deploymentpendingpreparedstackoverrideazure.md)[] | :heavy_minus_sign:                                                                                               | Azure permission configurations                                                                                  |
| `gcp`                                                                                                            | [models.DeploymentPendingPreparedStackOverrideGcp](../models/deploymentpendingpreparedstackoverridegcp.md)[]     | :heavy_minus_sign:                                                                                               | GCP permission configurations                                                                                    |
