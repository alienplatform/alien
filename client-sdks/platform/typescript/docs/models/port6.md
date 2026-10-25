# Port6

## Example Usage

```typescript
import { Port6 } from "@alienplatform/platform-api/models";

let value: Port6 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `secretRef`                                          | [models.PortSecretRef6](../models/portsecretref6.md) | :heavy_check_mark:                                   | Reference to a Kubernetes Secret                     |