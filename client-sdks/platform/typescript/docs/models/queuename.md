# QueueName

## Example Usage

```typescript
import { QueueName } from "@alienplatform/platform-api/models";

let value: QueueName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `secretRef`                                                  | [models.QueueNameSecretRef](../models/queuenamesecretref.md) | :heavy_check_mark:                                           | Reference to a Kubernetes Secret                             |