# Port5

## Example Usage

```typescript
import { Port5 } from "@alienplatform/platform-api/models";

let value: Port5 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `secretRef`                                          | [models.PortSecretRef5](../models/portsecretref5.md) | :heavy_check_mark:                                   | Reference to a Kubernetes Secret                     |