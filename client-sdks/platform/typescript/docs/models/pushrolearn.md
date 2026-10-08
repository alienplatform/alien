# PushRoleArn

## Example Usage

```typescript
import { PushRoleArn } from "@alienplatform/platform-api/models";

let value: PushRoleArn = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `secretRef`                                                      | [models.PushRoleArnSecretRef](../models/pushrolearnsecretref.md) | :heavy_check_mark:                                               | Reference to a Kubernetes Secret                                 |
