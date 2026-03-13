# DeploymentDetailResponseOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { DeploymentDetailResponseOverridePlatforms } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseOverridePlatforms = {};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                | [models.DeploymentDetailResponseOverrideAw](../models/deploymentdetailresponseoverrideaw.md)[]       | :heavy_minus_sign:                                                                                   | AWS permission configurations                                                                        |
| `azure`                                                                                              | [models.DeploymentDetailResponseOverrideAzure](../models/deploymentdetailresponseoverrideazure.md)[] | :heavy_minus_sign:                                                                                   | Azure permission configurations                                                                      |
| `gcp`                                                                                                | [models.DeploymentDetailResponseOverrideGcp](../models/deploymentdetailresponseoverridegcp.md)[]     | :heavy_minus_sign:                                                                                   | GCP permission configurations                                                                        |