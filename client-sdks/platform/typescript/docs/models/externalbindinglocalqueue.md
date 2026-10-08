# ExternalBindingLocalQueue

Local queue parameters

## Example Usage

```typescript
import { ExternalBindingLocalQueue } from "@alienplatform/platform-api/models";

let value: ExternalBindingLocalQueue = {
  queuePath: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  service: "local-queue",
  type: "queue",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `queuePath`                                                                                                          | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"local-queue"*                                                                                                      | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeQueue4](../models/typequeue4.md)                                                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
