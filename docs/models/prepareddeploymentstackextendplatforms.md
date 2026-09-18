# PreparedDeploymentStackExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { PreparedDeploymentStackExtendPlatforms } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackExtendPlatforms = {};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `aws`                                                                                          | [models.PreparedDeploymentStackExtendAw](../models/prepareddeploymentstackextendaw.md)[]       | :heavy_minus_sign:                                                                             | AWS permission configurations                                                                  |
| `azure`                                                                                        | [models.PreparedDeploymentStackExtendAzure](../models/prepareddeploymentstackextendazure.md)[] | :heavy_minus_sign:                                                                             | Azure permission configurations                                                                |
| `gcp`                                                                                          | [models.PreparedDeploymentStackExtendGcp](../models/prepareddeploymentstackextendgcp.md)[]     | :heavy_minus_sign:                                                                             | GCP permission configurations                                                                  |