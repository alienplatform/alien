# DeploymentOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentOverridePlatforms } from "@aliendotdev/platform-api/models";

let value: DeploymentOverridePlatforms = {};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `aws`                                                                    | [models.DeploymentOverrideAw](../models/deploymentoverrideaw.md)[]       | :heavy_minus_sign:                                                       | AWS permission configurations                                            |
| `azure`                                                                  | [models.DeploymentOverrideAzure](../models/deploymentoverrideazure.md)[] | :heavy_minus_sign:                                                       | Azure permission configurations                                          |
| `gcp`                                                                    | [models.DeploymentOverrideGcp](../models/deploymentoverridegcp.md)[]     | :heavy_minus_sign:                                                       | GCP permission configurations                                            |