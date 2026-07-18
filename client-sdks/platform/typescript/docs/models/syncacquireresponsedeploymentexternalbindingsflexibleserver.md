# SyncAcquireResponseDeploymentExternalBindingsFlexibleServer

Azure Flexible Server binding.

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentExternalBindingsFlexibleServer } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentExternalBindingsFlexibleServer = {
  service: "flexible-server",
  type: "postgres",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `database`                                                                                                           | *models.SyncAcquireResponseDeploymentDatabaseUnion3*                                                                 | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `host`                                                                                                               | *models.SyncAcquireResponseDeploymentHostUnion2*                                                                     | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `passwordSecretUri`                                                                                                  | *models.SyncAcquireResponseDeploymentPasswordSecretUriUnion*                                                         | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `port`                                                                                                               | *models.SyncAcquireResponseDeploymentPortUnion3*                                                                     | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `username`                                                                                                           | *models.SyncAcquireResponseDeploymentUsernameUnion3*                                                                 | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"flexible-server"*                                                                                                  | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseDeploymentTypePostgres3](../models/syncacquireresponsedeploymenttypepostgres3.md)         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |