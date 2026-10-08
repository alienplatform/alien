# Topic

## Example Usage

```typescript
import { Topic } from "@alienplatform/platform-api/models";

let value: Topic = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `secretRef`                                          | [models.TopicSecretRef](../models/topicsecretref.md) | :heavy_check_mark:                                   | Reference to a Kubernetes Secret                     |
