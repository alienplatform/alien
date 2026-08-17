# TargetDeploymentExternalBindingsPubsub

GCP Pub/Sub parameters

## Example Usage

```typescript
import { TargetDeploymentExternalBindingsPubsub } from "@alienplatform/platform-api/models";

let value: TargetDeploymentExternalBindingsPubsub = {
  service: "pubsub",
  type: "queue",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `subscription`                                                                                                       | *models.TargetDeploymentSubscriptionUnion*                                                                           | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `topic`                                                                                                              | *models.TargetDeploymentTopicUnion*                                                                                  | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"pubsub"*                                                                                                           | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.ConfigTypeQueue2](../models/configtypequeue2.md)                                                             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |