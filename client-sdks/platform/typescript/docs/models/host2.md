# Host2

## Example Usage

```typescript
import { Host2 } from "@alienplatform/platform-api/models";

let value: Host2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `secretRef`                                          | [models.HostSecretRef2](../models/hostsecretref2.md) | :heavy_check_mark:                                   | Reference to a Kubernetes Secret                     |
