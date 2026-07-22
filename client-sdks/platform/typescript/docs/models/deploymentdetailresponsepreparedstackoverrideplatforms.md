# DeploymentDetailResponsePreparedStackOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentDetailResponsePreparedStackOverridePlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePreparedStackOverridePlatforms = {};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                                          | [models.DeploymentDetailResponsePreparedStackOverrideAw](../models/deploymentdetailresponsepreparedstackoverrideaw.md)[]       | :heavy_minus_sign:                                                                                                             | AWS permission configurations                                                                                                  |
| `azure`                                                                                                                        | [models.DeploymentDetailResponsePreparedStackOverrideAzure](../models/deploymentdetailresponsepreparedstackoverrideazure.md)[] | :heavy_minus_sign:                                                                                                             | Azure permission configurations                                                                                                |
| `gcp`                                                                                                                          | [models.DeploymentDetailResponsePreparedStackOverrideGcp](../models/deploymentdetailresponsepreparedstackoverridegcp.md)[]     | :heavy_minus_sign:                                                                                                             | GCP permission configurations                                                                                                  |
