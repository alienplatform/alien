# StaticIp

## Example Usage

```typescript
import { StaticIp } from "@alienplatform/platform-api/models";

let value: StaticIp = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `secretRef`                                                | [models.StaticIpSecretRef](../models/staticipsecretref.md) | :heavy_check_mark:                                         | Reference to a Kubernetes Secret                           |
