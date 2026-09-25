# CreateChildDeploymentRequestOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { CreateChildDeploymentRequestOverridePlatforms } from "@alienplatform/platform-api/models";

let value: CreateChildDeploymentRequestOverridePlatforms = {};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                        | [models.CreateChildDeploymentRequestOverrideAw](../models/createchilddeploymentrequestoverrideaw.md)[]       | :heavy_minus_sign:                                                                                           | AWS permission configurations                                                                                |
| `azure`                                                                                                      | [models.CreateChildDeploymentRequestOverrideAzure](../models/createchilddeploymentrequestoverrideazure.md)[] | :heavy_minus_sign:                                                                                           | Azure permission configurations                                                                              |
| `gcp`                                                                                                        | [models.CreateChildDeploymentRequestOverrideGcp](../models/createchilddeploymentrequestoverridegcp.md)[]     | :heavy_minus_sign:                                                                                           | GCP permission configurations                                                                                |