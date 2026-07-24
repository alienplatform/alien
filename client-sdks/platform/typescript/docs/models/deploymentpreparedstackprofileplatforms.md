# DeploymentPreparedStackProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentPreparedStackProfilePlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentPreparedStackProfilePlatforms = {};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `aws`                                                                                            | [models.DeploymentPreparedStackProfileAw](../models/deploymentpreparedstackprofileaw.md)[]       | :heavy_minus_sign:                                                                               | AWS permission configurations                                                                    |
| `azure`                                                                                          | [models.DeploymentPreparedStackProfileAzure](../models/deploymentpreparedstackprofileazure.md)[] | :heavy_minus_sign:                                                                               | Azure permission configurations                                                                  |
| `gcp`                                                                                            | [models.DeploymentPreparedStackProfileGcp](../models/deploymentpreparedstackprofilegcp.md)[]     | :heavy_minus_sign:                                                                               | GCP permission configurations                                                                    |
