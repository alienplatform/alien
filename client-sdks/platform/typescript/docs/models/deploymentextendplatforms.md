# DeploymentExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentExtendPlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentExtendPlatforms = {};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `aws`                                                                | [models.DeploymentExtendAw](../models/deploymentextendaw.md)[]       | :heavy_minus_sign:                                                   | AWS permission configurations                                        |
| `azure`                                                              | [models.DeploymentExtendAzure](../models/deploymentextendazure.md)[] | :heavy_minus_sign:                                                   | Azure permission configurations                                      |
| `gcp`                                                                | [models.DeploymentExtendGcp](../models/deploymentextendgcp.md)[]     | :heavy_minus_sign:                                                   | GCP permission configurations                                        |