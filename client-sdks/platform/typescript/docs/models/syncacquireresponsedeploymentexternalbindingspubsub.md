# SyncAcquireResponseDeploymentExternalBindingsPubsub

GCP Pub/Sub parameters

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentExternalBindingsPubsub } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentExternalBindingsPubsub = {
  service: "pubsub",
  type: "queue",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `subscription`                                                                                                       | *models.SyncAcquireResponseDeploymentSubscriptionUnion*                                                              | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `topic`                                                                                                              | *models.SyncAcquireResponseDeploymentTopicUnion*                                                                     | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"pubsub"*                                                                                                           | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseDeploymentTypeQueue2](../models/syncacquireresponsedeploymenttypequeue2.md)               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |