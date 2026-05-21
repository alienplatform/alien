# PreparedDeploymentStackProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { PreparedDeploymentStackProfilePlatforms } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackProfilePlatforms = {};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `aws`                                                                                            | [models.PreparedDeploymentStackProfileAw](../models/prepareddeploymentstackprofileaw.md)[]       | :heavy_minus_sign:                                                                               | AWS permission configurations                                                                    |
| `azure`                                                                                          | [models.PreparedDeploymentStackProfileAzure](../models/prepareddeploymentstackprofileazure.md)[] | :heavy_minus_sign:                                                                               | Azure permission configurations                                                                  |
| `gcp`                                                                                            | [models.PreparedDeploymentStackProfileGcp](../models/prepareddeploymentstackprofilegcp.md)[]     | :heavy_minus_sign:                                                                               | GCP permission configurations                                                                    |