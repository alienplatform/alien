# DeploymentConfigExternalBindingsS3

AWS S3 storage binding configuration

## Example Usage

```typescript
import { DeploymentConfigExternalBindingsS3 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigExternalBindingsS3 = {
  service: "s3",
  type: "storage",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `bucketName`                                                                                                         | *models.DeploymentConfigBucketNameUnion1*                                                                            | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"s3"*                                                                                                               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.DeploymentConfigTypeStorage1](../models/deploymentconfigtypestorage1.md)                                     | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |