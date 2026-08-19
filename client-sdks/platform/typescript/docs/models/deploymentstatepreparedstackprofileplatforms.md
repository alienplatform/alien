# DeploymentStatePreparedStackProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentStatePreparedStackProfilePlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentStatePreparedStackProfilePlatforms = {};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                      | [models.DeploymentStatePreparedStackProfileAw](../models/deploymentstatepreparedstackprofileaw.md)[]       | :heavy_minus_sign:                                                                                         | AWS permission configurations                                                                              |
| `azure`                                                                                                    | [models.DeploymentStatePreparedStackProfileAzure](../models/deploymentstatepreparedstackprofileazure.md)[] | :heavy_minus_sign:                                                                                         | Azure permission configurations                                                                            |
| `gcp`                                                                                                      | [models.DeploymentStatePreparedStackProfileGcp](../models/deploymentstatepreparedstackprofilegcp.md)[]     | :heavy_minus_sign:                                                                                         | GCP permission configurations                                                                              |