# DeploymentProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentProfilePlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentProfilePlatforms = {};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `aws`                                                                  | [models.DeploymentProfileAw](../models/deploymentprofileaw.md)[]       | :heavy_minus_sign:                                                     | AWS permission configurations                                          |
| `azure`                                                                | [models.DeploymentProfileAzure](../models/deploymentprofileazure.md)[] | :heavy_minus_sign:                                                     | Azure permission configurations                                        |
| `gcp`                                                                  | [models.DeploymentProfileGcp](../models/deploymentprofilegcp.md)[]     | :heavy_minus_sign:                                                     | GCP permission configurations                                          |