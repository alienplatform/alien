# SyncAcquireResponseExternalBindingsPubsub

GCP Pub/Sub parameters

## Example Usage

```typescript
import { SyncAcquireResponseExternalBindingsPubsub } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseExternalBindingsPubsub = {
  service: "pubsub",
  type: "queue",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `subscription`                                                                                                       | *models.SyncAcquireResponseSubscriptionUnion*                                                                        | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `topic`                                                                                                              | *models.SyncAcquireResponseTopicUnion*                                                                               | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"pubsub"*                                                                                                           | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseTypeQueue2](../models/syncacquireresponsetypequeue2.md)                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |