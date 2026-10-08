# PullRoleArn

## Example Usage

```typescript
import { PullRoleArn } from "@alienplatform/platform-api/models";

let value: PullRoleArn = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `secretRef`                                                      | [models.PullRoleArnSecretRef](../models/pullrolearnsecretref.md) | :heavy_check_mark:                                               | Reference to a Kubernetes Secret                                 |
