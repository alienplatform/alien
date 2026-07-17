# SyncAcquireResponseDeploymentExternalBindingsAurora

AWS Aurora Serverless v2 binding.

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentExternalBindingsAurora } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentExternalBindingsAurora = {
  service: "aurora",
  type: "postgres",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `clusterEndpoint`                                                                                                    | *models.SyncAcquireResponseDeploymentClusterEndpointUnion*                                                           | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `database`                                                                                                           | *models.SyncAcquireResponseDeploymentDatabaseUnion1*                                                                 | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `passwordSecretArn`                                                                                                  | *models.SyncAcquireResponseDeploymentPasswordSecretArnUnion*                                                         | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `port`                                                                                                               | *models.SyncAcquireResponseDeploymentPortUnion1*                                                                     | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `username`                                                                                                           | *models.SyncAcquireResponseDeploymentUsernameUnion1*                                                                 | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"aurora"*                                                                                                           | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseDeploymentTypePostgres1](../models/syncacquireresponsedeploymenttypepostgres1.md)         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |