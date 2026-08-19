# DeploymentStatePreparedStackOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentStatePreparedStackOverridePlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentStatePreparedStackOverridePlatforms = {};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                        | [models.DeploymentStatePreparedStackOverrideAw](../models/deploymentstatepreparedstackoverrideaw.md)[]       | :heavy_minus_sign:                                                                                           | AWS permission configurations                                                                                |
| `azure`                                                                                                      | [models.DeploymentStatePreparedStackOverrideAzure](../models/deploymentstatepreparedstackoverrideazure.md)[] | :heavy_minus_sign:                                                                                           | Azure permission configurations                                                                              |
| `gcp`                                                                                                        | [models.DeploymentStatePreparedStackOverrideGcp](../models/deploymentstatepreparedstackoverridegcp.md)[]     | :heavy_minus_sign:                                                                                           | GCP permission configurations                                                                                |