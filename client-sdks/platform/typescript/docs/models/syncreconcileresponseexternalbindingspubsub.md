# SyncReconcileResponseExternalBindingsPubsub

GCP Pub/Sub parameters

## Example Usage

```typescript
import { SyncReconcileResponseExternalBindingsPubsub } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseExternalBindingsPubsub = {
  service: "pubsub",
  type: "queue",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `subscription`                                                                                                       | *models.SyncReconcileResponseSubscriptionUnion*                                                                      | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `topic`                                                                                                              | *models.SyncReconcileResponseTopicUnion*                                                                             | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"pubsub"*                                                                                                           | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncReconcileResponseTypeQueue2](../models/syncreconcileresponsetypequeue2.md)                               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |