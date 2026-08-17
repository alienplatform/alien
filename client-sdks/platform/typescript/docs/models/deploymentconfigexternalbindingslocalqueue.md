# DeploymentConfigExternalBindingsLocalQueue

Local queue parameters

## Example Usage

```typescript
import { DeploymentConfigExternalBindingsLocalQueue } from "@alienplatform/platform-api/models";

let value: DeploymentConfigExternalBindingsLocalQueue = {
  service: "local-queue",
  type: "queue",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `queuePath`                                                                                                          | *models.DeploymentConfigQueuePathUnion*                                                                              | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"local-queue"*                                                                                                      | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.DeploymentConfigTypeQueue4](../models/deploymentconfigtypequeue4.md)                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |