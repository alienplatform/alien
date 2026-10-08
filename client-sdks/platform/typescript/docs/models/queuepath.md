# QueuePath

## Example Usage

```typescript
import { QueuePath } from "@alienplatform/platform-api/models";

let value: QueuePath = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `secretRef`                                                  | [models.QueuePathSecretRef](../models/queuepathsecretref.md) | :heavy_check_mark:                                           | Reference to a Kubernetes Secret                             |
