# DeploymentConfigExternalBindingsPubsub

GCP Pub/Sub parameters

## Example Usage

```typescript
import { DeploymentConfigExternalBindingsPubsub } from "@alienplatform/platform-api/models";

let value: DeploymentConfigExternalBindingsPubsub = {
  service: "pubsub",
  type: "queue",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `subscription`                                                                                                       | *models.DeploymentConfigSubscriptionUnion*                                                                           | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `topic`                                                                                                              | *models.DeploymentConfigTopicUnion*                                                                                  | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"pubsub"*                                                                                                           | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.DeploymentConfigTypeQueue2](../models/deploymentconfigtypequeue2.md)                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |