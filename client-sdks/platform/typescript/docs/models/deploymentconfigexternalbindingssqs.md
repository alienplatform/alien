# DeploymentConfigExternalBindingsSqs

AWS SQS queue parameters

## Example Usage

```typescript
import { DeploymentConfigExternalBindingsSqs } from "@alienplatform/platform-api/models";

let value: DeploymentConfigExternalBindingsSqs = {
  service: "sqs",
  type: "queue",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `queueUrl`                                                                                                           | *models.DeploymentConfigQueueUrlUnion*                                                                               | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"sqs"*                                                                                                              | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.DeploymentConfigTypeQueue1](../models/deploymentconfigtypequeue1.md)                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |