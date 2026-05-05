# SyncAcquireResponseExternalBindingsLocalQueue

Local queue parameters

## Example Usage

```typescript
import { SyncAcquireResponseExternalBindingsLocalQueue } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseExternalBindingsLocalQueue = {
  service: "local-queue",
  type: "queue",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `queuePath`                                                                                                          | *models.SyncAcquireResponseQueuePathUnion*                                                                           | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"local-queue"*                                                                                                      | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseTypeQueue4](../models/syncacquireresponsetypequeue4.md)                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |