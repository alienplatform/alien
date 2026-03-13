# DeploymentDetailResponseExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentDetailResponseExtendPlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseExtendPlatforms = {};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `aws`                                                                                            | [models.DeploymentDetailResponseExtendAw](../models/deploymentdetailresponseextendaw.md)[]       | :heavy_minus_sign:                                                                               | AWS permission configurations                                                                    |
| `azure`                                                                                          | [models.DeploymentDetailResponseExtendAzure](../models/deploymentdetailresponseextendazure.md)[] | :heavy_minus_sign:                                                                               | Azure permission configurations                                                                  |
| `gcp`                                                                                            | [models.DeploymentDetailResponseExtendGcp](../models/deploymentdetailresponseextendgcp.md)[]     | :heavy_minus_sign:                                                                               | GCP permission configurations                                                                    |