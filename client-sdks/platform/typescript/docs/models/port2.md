# Port2

## Example Usage

```typescript
import { Port2 } from "@alienplatform/platform-api/models";

let value: Port2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `secretRef`                                          | [models.PortSecretRef2](../models/portsecretref2.md) | :heavy_check_mark:                                   | Reference to a Kubernetes Secret                     |
