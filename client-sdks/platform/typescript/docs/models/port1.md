# Port1

## Example Usage

```typescript
import { Port1 } from "@alienplatform/platform-api/models";

let value: Port1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `secretRef`                                          | [models.PortSecretRef1](../models/portsecretref1.md) | :heavy_check_mark:                                   | Reference to a Kubernetes Secret                     |