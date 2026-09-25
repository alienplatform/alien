# CreateChildDeploymentRequestProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { CreateChildDeploymentRequestProfilePlatforms } from "@alienplatform/platform-api/models";

let value: CreateChildDeploymentRequestProfilePlatforms = {};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                      | [models.CreateChildDeploymentRequestProfileAw](../models/createchilddeploymentrequestprofileaw.md)[]       | :heavy_minus_sign:                                                                                         | AWS permission configurations                                                                              |
| `azure`                                                                                                    | [models.CreateChildDeploymentRequestProfileAzure](../models/createchilddeploymentrequestprofileazure.md)[] | :heavy_minus_sign:                                                                                         | Azure permission configurations                                                                            |
| `gcp`                                                                                                      | [models.CreateChildDeploymentRequestProfileGcp](../models/createchilddeploymentrequestprofilegcp.md)[]     | :heavy_minus_sign:                                                                                         | GCP permission configurations                                                                              |