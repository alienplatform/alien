# DeploymentStatePendingPreparedStackProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentStatePendingPreparedStackProfilePlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentStatePendingPreparedStackProfilePlatforms = {};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                                    | [models.DeploymentStatePendingPreparedStackProfileAw](../models/deploymentstatependingpreparedstackprofileaw.md)[]       | :heavy_minus_sign:                                                                                                       | AWS permission configurations                                                                                            |
| `azure`                                                                                                                  | [models.DeploymentStatePendingPreparedStackProfileAzure](../models/deploymentstatependingpreparedstackprofileazure.md)[] | :heavy_minus_sign:                                                                                                       | Azure permission configurations                                                                                          |
| `gcp`                                                                                                                    | [models.DeploymentStatePendingPreparedStackProfileGcp](../models/deploymentstatependingpreparedstackprofilegcp.md)[]     | :heavy_minus_sign:                                                                                                       | GCP permission configurations                                                                                            |