# SyncReconcileResponseStaticIp

## Example Usage

```typescript
import { SyncReconcileResponseStaticIp } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseStaticIp = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                          | [models.SyncReconcileResponseStaticIpSecretRef](../models/syncreconcileresponsestaticipsecretref.md) | :heavy_check_mark:                                                                                   | Reference to a Kubernetes Secret                                                                     |