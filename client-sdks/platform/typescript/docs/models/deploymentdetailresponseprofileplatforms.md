# DeploymentDetailResponseProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentDetailResponseProfilePlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseProfilePlatforms = {};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `aws`                                                                                              | [models.DeploymentDetailResponseProfileAw](../models/deploymentdetailresponseprofileaw.md)[]       | :heavy_minus_sign:                                                                                 | AWS permission configurations                                                                      |
| `azure`                                                                                            | [models.DeploymentDetailResponseProfileAzure](../models/deploymentdetailresponseprofileazure.md)[] | :heavy_minus_sign:                                                                                 | Azure permission configurations                                                                    |
| `gcp`                                                                                              | [models.DeploymentDetailResponseProfileGcp](../models/deploymentdetailresponseprofilegcp.md)[]     | :heavy_minus_sign:                                                                                 | GCP permission configurations                                                                      |