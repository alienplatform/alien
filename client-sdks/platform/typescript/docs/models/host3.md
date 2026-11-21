# Host3

## Example Usage

```typescript
import { Host3 } from "@alienplatform/platform-api/models";

let value: Host3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `secretRef`                                          | [models.HostSecretRef3](../models/hostsecretref3.md) | :heavy_check_mark:                                   | Reference to a Kubernetes Secret                     |