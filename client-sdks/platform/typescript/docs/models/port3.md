# Port3

## Example Usage

```typescript
import { Port3 } from "@alienplatform/platform-api/models";

let value: Port3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `secretRef`                                          | [models.PortSecretRef3](../models/portsecretref3.md) | :heavy_check_mark:                                   | Reference to a Kubernetes Secret                     |