# Region1

## Example Usage

```typescript
import { Region1 } from "@alienplatform/platform-api/models";

let value: Region1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `secretRef`                                              | [models.RegionSecretRef1](../models/regionsecretref1.md) | :heavy_check_mark:                                       | Reference to a Kubernetes Secret                         |
