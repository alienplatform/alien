# TargetDeploymentExternalBindingsServicebus

Azure Service Bus parameters

## Example Usage

```typescript
import { TargetDeploymentExternalBindingsServicebus } from "@alienplatform/platform-api/models";

let value: TargetDeploymentExternalBindingsServicebus = {
  service: "servicebus",
  type: "queue",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `namespace`                                                                                                          | *models.TargetDeploymentNamespaceUnion1*                                                                             | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `queueName`                                                                                                          | *models.TargetDeploymentQueueNameUnion*                                                                              | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"servicebus"*                                                                                                       | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TargetDeploymentTypeQueue3](../models/targetdeploymenttypequeue3.md)                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |