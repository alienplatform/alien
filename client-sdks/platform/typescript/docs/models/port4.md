# Port4

## Example Usage

```typescript
import { Port4 } from "@alienplatform/platform-api/models";

let value: Port4 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `secretRef`                                          | [models.PortSecretRef4](../models/portsecretref4.md) | :heavy_check_mark:                                   | Reference to a Kubernetes Secret                     |