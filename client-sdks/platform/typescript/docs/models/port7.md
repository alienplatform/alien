# Port7

## Example Usage

```typescript
import { Port7 } from "@alienplatform/platform-api/models";

let value: Port7 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `secretRef`                                          | [models.PortSecretRef7](../models/portsecretref7.md) | :heavy_check_mark:                                   | Reference to a Kubernetes Secret                     |