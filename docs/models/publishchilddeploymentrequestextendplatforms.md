# PublishChildDeploymentRequestExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { PublishChildDeploymentRequestExtendPlatforms } from "@alienplatform/platform-api/models";

let value: PublishChildDeploymentRequestExtendPlatforms = {};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                      | [models.PublishChildDeploymentRequestExtendAw](../models/publishchilddeploymentrequestextendaw.md)[]       | :heavy_minus_sign:                                                                                         | AWS permission configurations                                                                              |
| `azure`                                                                                                    | [models.PublishChildDeploymentRequestExtendAzure](../models/publishchilddeploymentrequestextendazure.md)[] | :heavy_minus_sign:                                                                                         | Azure permission configurations                                                                            |
| `gcp`                                                                                                      | [models.PublishChildDeploymentRequestExtendGcp](../models/publishchilddeploymentrequestextendgcp.md)[]     | :heavy_minus_sign:                                                                                         | GCP permission configurations                                                                              |