# SyncAcquireResponseDeploymentExternalBindingsLocalQueue

Local queue parameters

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentExternalBindingsLocalQueue } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentExternalBindingsLocalQueue = {
  service: "local-queue",
  type: "queue",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `queuePath`                                                                                                          | *models.SyncAcquireResponseDeploymentQueuePathUnion*                                                                 | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"local-queue"*                                                                                                      | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseDeploymentTypeQueue4](../models/syncacquireresponsedeploymenttypequeue4.md)               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |