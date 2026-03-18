# SyncAcquireResponseExternalBindingsServicebus

Azure Service Bus parameters

## Example Usage

```typescript
import { SyncAcquireResponseExternalBindingsServicebus } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseExternalBindingsServicebus = {
  service: "servicebus",
  type: "queue",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `namespace`                                                                                                          | *models.SyncAcquireResponseNamespaceUnion1*                                                                          | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `queueName`                                                                                                          | *models.SyncAcquireResponseQueueNameUnion*                                                                           | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"servicebus"*                                                                                                       | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseTypeQueue3](../models/syncacquireresponsetypequeue3.md)                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |