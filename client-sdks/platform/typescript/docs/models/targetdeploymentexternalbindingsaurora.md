# TargetDeploymentExternalBindingsAurora

AWS Aurora Serverless v2 binding.

## Example Usage

```typescript
import { TargetDeploymentExternalBindingsAurora } from "@alienplatform/platform-api/models";

let value: TargetDeploymentExternalBindingsAurora = {
  service: "aurora",
  type: "postgres",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `clusterEndpoint`                                                                                                    | *models.TargetDeploymentClusterEndpointUnion*                                                                        | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `database`                                                                                                           | *models.TargetDeploymentDatabaseUnion1*                                                                              | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `passwordSecretArn`                                                                                                  | *models.TargetDeploymentPasswordSecretArnUnion*                                                                      | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `port`                                                                                                               | *models.TargetDeploymentPortUnion1*                                                                                  | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `username`                                                                                                           | *models.TargetDeploymentUsernameUnion1*                                                                              | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"aurora"*                                                                                                           | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TargetDeploymentTypePostgres1](../models/targetdeploymenttypepostgres1.md)                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |