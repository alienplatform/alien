# ExternalBindingLocalQueue

Local development queue binding

## Example Usage

```typescript
import { ExternalBindingLocalQueue } from "@alienplatform/manager-api/models";

let value: ExternalBindingLocalQueue = {
  queuePath: {
    secretRef: {
      key: "<key>",
      name: "<value>",
    },
  },
  service: "local-queue",
  type: "queue",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `queuePath`                                                                                                          | *models.BindingValueStringUnion*                                                                                     | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"local-queue"*                                                                                                      | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeQueue4](../models/typequeue4.md)                                                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |