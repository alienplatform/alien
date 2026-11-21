# Port11

## Example Usage

```typescript
import { Port11 } from "@alienplatform/platform-api/models";

let value: Port11 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `secretRef`                                            | [models.PortSecretRef11](../models/portsecretref11.md) | :heavy_check_mark:                                     | Reference to a Kubernetes Secret                       |