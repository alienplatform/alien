# Host1

## Example Usage

```typescript
import { Host1 } from "@alienplatform/platform-api/models";

let value: Host1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `secretRef`                                          | [models.HostSecretRef1](../models/hostsecretref1.md) | :heavy_check_mark:                                   | Reference to a Kubernetes Secret                     |