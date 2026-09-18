# TargetDeploymentExternalBindingsSqs

AWS SQS queue parameters

## Example Usage

```typescript
import { TargetDeploymentExternalBindingsSqs } from "@alienplatform/platform-api/models";

let value: TargetDeploymentExternalBindingsSqs = {
  service: "sqs",
  type: "queue",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `queueUrl`                                                                                                           | *models.TargetDeploymentQueueUrlUnion*                                                                               | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"sqs"*                                                                                                              | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TargetDeploymentTypeQueue1](../models/targetdeploymenttypequeue1.md)                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |