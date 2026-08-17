# TargetDeploymentExternalBindingsLocalQueue

Local queue parameters

## Example Usage

```typescript
import { TargetDeploymentExternalBindingsLocalQueue } from "@alienplatform/platform-api/models";

let value: TargetDeploymentExternalBindingsLocalQueue = {
  service: "local-queue",
  type: "queue",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `queuePath`                                                                                                          | *models.TargetDeploymentQueuePathUnion*                                                                              | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"local-queue"*                                                                                                      | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.ConfigTypeQueue4](../models/configtypequeue4.md)                                                             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |