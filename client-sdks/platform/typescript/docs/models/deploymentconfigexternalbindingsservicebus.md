# DeploymentConfigExternalBindingsServicebus

Azure Service Bus parameters

## Example Usage

```typescript
import { DeploymentConfigExternalBindingsServicebus } from "@alienplatform/platform-api/models";

let value: DeploymentConfigExternalBindingsServicebus = {
  service: "servicebus",
  type: "queue",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `namespace`                                                                                                          | *models.DeploymentConfigNamespaceUnion1*                                                                             | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `queueName`                                                                                                          | *models.DeploymentConfigQueueNameUnion*                                                                              | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"servicebus"*                                                                                                       | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.DeploymentConfigTypeQueue3](../models/deploymentconfigtypequeue3.md)                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |