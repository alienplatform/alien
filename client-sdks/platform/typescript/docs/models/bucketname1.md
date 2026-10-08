# BucketName1

## Example Usage

```typescript
import { BucketName1 } from "@alienplatform/platform-api/models";

let value: BucketName1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `secretRef`                                                      | [models.BucketNameSecretRef1](../models/bucketnamesecretref1.md) | :heavy_check_mark:                                               | Reference to a Kubernetes Secret                                 |
