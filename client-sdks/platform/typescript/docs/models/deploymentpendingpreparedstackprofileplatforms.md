# DeploymentPendingPreparedStackProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentPendingPreparedStackProfilePlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentPendingPreparedStackProfilePlatforms = {};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                          | [models.DeploymentPendingPreparedStackProfileAw](../models/deploymentpendingpreparedstackprofileaw.md)[]       | :heavy_minus_sign:                                                                                             | AWS permission configurations                                                                                  |
| `azure`                                                                                                        | [models.DeploymentPendingPreparedStackProfileAzure](../models/deploymentpendingpreparedstackprofileazure.md)[] | :heavy_minus_sign:                                                                                             | Azure permission configurations                                                                                |
| `gcp`                                                                                                          | [models.DeploymentPendingPreparedStackProfileGcp](../models/deploymentpendingpreparedstackprofilegcp.md)[]     | :heavy_minus_sign:                                                                                             | GCP permission configurations                                                                                  |
