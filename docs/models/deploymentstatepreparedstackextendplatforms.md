# DeploymentStatePreparedStackExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentStatePreparedStackExtendPlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentStatePreparedStackExtendPlatforms = {};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                    | [models.DeploymentStatePreparedStackExtendAw](../models/deploymentstatepreparedstackextendaw.md)[]       | :heavy_minus_sign:                                                                                       | AWS permission configurations                                                                            |
| `azure`                                                                                                  | [models.DeploymentStatePreparedStackExtendAzure](../models/deploymentstatepreparedstackextendazure.md)[] | :heavy_minus_sign:                                                                                       | Azure permission configurations                                                                          |
| `gcp`                                                                                                    | [models.DeploymentStatePreparedStackExtendGcp](../models/deploymentstatepreparedstackextendgcp.md)[]     | :heavy_minus_sign:                                                                                       | GCP permission configurations                                                                            |