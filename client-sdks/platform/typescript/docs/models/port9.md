# Port9

## Example Usage

```typescript
import { Port9 } from "@alienplatform/platform-api/models";

let value: Port9 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `secretRef`                                          | [models.PortSecretRef9](../models/portsecretref9.md) | :heavy_check_mark:                                   | Reference to a Kubernetes Secret                     |