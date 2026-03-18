# SyncReconcileResponseExternalBindingsServicebus

Azure Service Bus parameters

## Example Usage

```typescript
import { SyncReconcileResponseExternalBindingsServicebus } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseExternalBindingsServicebus = {
  service: "servicebus",
  type: "queue",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `namespace`                                                                                                          | *models.SyncReconcileResponseNamespaceUnion1*                                                                        | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `queueName`                                                                                                          | *models.SyncReconcileResponseQueueNameUnion*                                                                         | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"servicebus"*                                                                                                       | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncReconcileResponseTypeQueue3](../models/syncreconcileresponsetypequeue3.md)                               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |