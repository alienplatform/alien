# Region2

## Example Usage

```typescript
import { Region2 } from "@alienplatform/platform-api/models";

let value: Region2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `secretRef`                                              | [models.RegionSecretRef2](../models/regionsecretref2.md) | :heavy_check_mark:                                       | Reference to a Kubernetes Secret                         |