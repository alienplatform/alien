# Port10

## Example Usage

```typescript
import { Port10 } from "@alienplatform/platform-api/models";

let value: Port10 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `secretRef`                                            | [models.PortSecretRef10](../models/portsecretref10.md) | :heavy_check_mark:                                     | Reference to a Kubernetes Secret                       |