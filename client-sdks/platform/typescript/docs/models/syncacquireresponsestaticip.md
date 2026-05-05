# SyncAcquireResponseStaticIp

## Example Usage

```typescript
import { SyncAcquireResponseStaticIp } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseStaticIp = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                      | [models.SyncAcquireResponseStaticIpSecretRef](../models/syncacquireresponsestaticipsecretref.md) | :heavy_check_mark:                                                                               | Reference to a Kubernetes Secret                                                                 |