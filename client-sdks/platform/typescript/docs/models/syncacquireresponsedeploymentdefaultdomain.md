# SyncAcquireResponseDeploymentDefaultDomain

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentDefaultDomain } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentDefaultDomain = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                                    | [models.SyncAcquireResponseDeploymentDefaultDomainSecretRef](../models/syncacquireresponsedeploymentdefaultdomainsecretref.md) | :heavy_check_mark:                                                                                                             | Reference to a Kubernetes Secret                                                                                               |