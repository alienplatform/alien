# DeploymentDetailResponsePreparedStackProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentDetailResponsePreparedStackProfilePlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePreparedStackProfilePlatforms = {};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                        | [models.DeploymentDetailResponsePreparedStackProfileAw](../models/deploymentdetailresponsepreparedstackprofileaw.md)[]       | :heavy_minus_sign:                                                                                                           | AWS permission configurations                                                                                                |
| `azure`                                                                                                                      | [models.DeploymentDetailResponsePreparedStackProfileAzure](../models/deploymentdetailresponsepreparedstackprofileazure.md)[] | :heavy_minus_sign:                                                                                                           | Azure permission configurations                                                                                              |
| `gcp`                                                                                                                        | [models.DeploymentDetailResponsePreparedStackProfileGcp](../models/deploymentdetailresponsepreparedstackprofilegcp.md)[]     | :heavy_minus_sign:                                                                                                           | GCP permission configurations                                                                                                |
