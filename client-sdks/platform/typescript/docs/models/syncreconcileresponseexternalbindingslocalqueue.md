# SyncReconcileResponseExternalBindingsLocalQueue

Local queue parameters

## Example Usage

```typescript
import { SyncReconcileResponseExternalBindingsLocalQueue } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseExternalBindingsLocalQueue = {
  service: "local-queue",
  type: "queue",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `queuePath`                                                                                                          | *models.SyncReconcileResponseQueuePathUnion*                                                                         | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"local-queue"*                                                                                                      | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TargetTypeQueue4](../models/targettypequeue4.md)                                                             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |