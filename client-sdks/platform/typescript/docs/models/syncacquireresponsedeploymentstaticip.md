# SyncAcquireResponseDeploymentStaticIp

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentStaticIp } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentStaticIp = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                          | [models.SyncAcquireResponseDeploymentStaticIpSecretRef](../models/syncacquireresponsedeploymentstaticipsecretref.md) | :heavy_check_mark:                                                                                                   | Reference to a Kubernetes Secret                                                                                     |