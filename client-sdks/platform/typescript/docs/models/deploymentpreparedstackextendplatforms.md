# DeploymentPreparedStackExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentPreparedStackExtendPlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentPreparedStackExtendPlatforms = {};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `aws`                                                                                          | [models.DeploymentPreparedStackExtendAw](../models/deploymentpreparedstackextendaw.md)[]       | :heavy_minus_sign:                                                                             | AWS permission configurations                                                                  |
| `azure`                                                                                        | [models.DeploymentPreparedStackExtendAzure](../models/deploymentpreparedstackextendazure.md)[] | :heavy_minus_sign:                                                                             | Azure permission configurations                                                                |
| `gcp`                                                                                          | [models.DeploymentPreparedStackExtendGcp](../models/deploymentpreparedstackextendgcp.md)[]     | :heavy_minus_sign:                                                                             | GCP permission configurations                                                                  |
