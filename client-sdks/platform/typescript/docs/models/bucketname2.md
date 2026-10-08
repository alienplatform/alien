# BucketName2

## Example Usage

```typescript
import { BucketName2 } from "@alienplatform/platform-api/models";

let value: BucketName2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `secretRef`                                                      | [models.BucketNameSecretRef2](../models/bucketnamesecretref2.md) | :heavy_check_mark:                                               | Reference to a Kubernetes Secret                                 |
