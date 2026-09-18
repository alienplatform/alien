# PreparedDeploymentStackOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { PreparedDeploymentStackOverridePlatforms } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackOverridePlatforms = {};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `aws`                                                                                              | [models.PreparedDeploymentStackOverrideAw](../models/prepareddeploymentstackoverrideaw.md)[]       | :heavy_minus_sign:                                                                                 | AWS permission configurations                                                                      |
| `azure`                                                                                            | [models.PreparedDeploymentStackOverrideAzure](../models/prepareddeploymentstackoverrideazure.md)[] | :heavy_minus_sign:                                                                                 | Azure permission configurations                                                                    |
| `gcp`                                                                                              | [models.PreparedDeploymentStackOverrideGcp](../models/prepareddeploymentstackoverridegcp.md)[]     | :heavy_minus_sign:                                                                                 | GCP permission configurations                                                                      |