# SyncAcquireResponseDeploymentExternalBindingsServicebus

Azure Service Bus parameters

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentExternalBindingsServicebus } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentExternalBindingsServicebus = {
  service: "servicebus",
  type: "queue",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `namespace`                                                                                                          | *models.SyncAcquireResponseDeploymentNamespaceUnion1*                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `queueName`                                                                                                          | *models.SyncAcquireResponseDeploymentQueueNameUnion*                                                                 | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"servicebus"*                                                                                                       | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseDeploymentTypeQueue3](../models/syncacquireresponsedeploymenttypequeue3.md)               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |