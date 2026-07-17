# SyncAcquireResponseDeploymentExternalBindingsSqs

AWS SQS queue parameters

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentExternalBindingsSqs } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentExternalBindingsSqs = {
  service: "sqs",
  type: "queue",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `queueUrl`                                                                                                           | *models.SyncAcquireResponseDeploymentQueueUrlUnion*                                                                  | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"sqs"*                                                                                                              | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseDeploymentTypeQueue1](../models/syncacquireresponsedeploymenttypequeue1.md)               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |