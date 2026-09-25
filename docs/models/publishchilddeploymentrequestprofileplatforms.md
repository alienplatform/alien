# PublishChildDeploymentRequestProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { PublishChildDeploymentRequestProfilePlatforms } from "@alienplatform/platform-api/models";

let value: PublishChildDeploymentRequestProfilePlatforms = {};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                        | [models.PublishChildDeploymentRequestProfileAw](../models/publishchilddeploymentrequestprofileaw.md)[]       | :heavy_minus_sign:                                                                                           | AWS permission configurations                                                                                |
| `azure`                                                                                                      | [models.PublishChildDeploymentRequestProfileAzure](../models/publishchilddeploymentrequestprofileazure.md)[] | :heavy_minus_sign:                                                                                           | Azure permission configurations                                                                              |
| `gcp`                                                                                                        | [models.PublishChildDeploymentRequestProfileGcp](../models/publishchilddeploymentrequestprofilegcp.md)[]     | :heavy_minus_sign:                                                                                           | GCP permission configurations                                                                                |