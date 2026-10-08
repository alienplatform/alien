# QueueUrl

## Example Usage

```typescript
import { QueueUrl } from "@alienplatform/platform-api/models";

let value: QueueUrl = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `secretRef`                                                | [models.QueueUrlSecretRef](../models/queueurlsecretref.md) | :heavy_check_mark:                                         | Reference to a Kubernetes Secret                           |
