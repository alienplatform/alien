# Port8

## Example Usage

```typescript
import { Port8 } from "@alienplatform/platform-api/models";

let value: Port8 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `secretRef`                                          | [models.PortSecretRef8](../models/portsecretref8.md) | :heavy_check_mark:                                   | Reference to a Kubernetes Secret                     |