# PublishChildDeploymentRequestOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { PublishChildDeploymentRequestOverridePlatforms } from "@alienplatform/platform-api/models";

let value: PublishChildDeploymentRequestOverridePlatforms = {};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                          | [models.PublishChildDeploymentRequestOverrideAw](../models/publishchilddeploymentrequestoverrideaw.md)[]       | :heavy_minus_sign:                                                                                             | AWS permission configurations                                                                                  |
| `azure`                                                                                                        | [models.PublishChildDeploymentRequestOverrideAzure](../models/publishchilddeploymentrequestoverrideazure.md)[] | :heavy_minus_sign:                                                                                             | Azure permission configurations                                                                                |
| `gcp`                                                                                                          | [models.PublishChildDeploymentRequestOverrideGcp](../models/publishchilddeploymentrequestoverridegcp.md)[]     | :heavy_minus_sign:                                                                                             | GCP permission configurations                                                                                  |