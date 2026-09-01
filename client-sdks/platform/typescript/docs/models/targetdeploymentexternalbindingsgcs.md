# TargetDeploymentExternalBindingsGcs

Google Cloud Storage binding configuration

## Example Usage

```typescript
import { TargetDeploymentExternalBindingsGcs } from "@alienplatform/platform-api/models";

let value: TargetDeploymentExternalBindingsGcs = {
  service: "gcs",
  type: "storage",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `bucketName`                                                                                                         | *models.TargetDeploymentBucketNameUnion2*                                                                            | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"gcs"*                                                                                                              | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TargetDeploymentTypeStorage3](../models/targetdeploymenttypestorage3.md)                                     | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |