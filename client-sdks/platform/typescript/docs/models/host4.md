# Host4

## Example Usage

```typescript
import { Host4 } from "@alienplatform/platform-api/models";

let value: Host4 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `secretRef`                                          | [models.HostSecretRef4](../models/hostsecretref4.md) | :heavy_check_mark:                                   | Reference to a Kubernetes Secret                     |