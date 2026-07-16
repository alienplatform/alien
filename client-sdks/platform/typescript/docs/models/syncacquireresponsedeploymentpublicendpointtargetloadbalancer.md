# SyncAcquireResponseDeploymentPublicEndpointTargetLoadBalancer

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPublicEndpointTargetLoadBalancer } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPublicEndpointTargetLoadBalancer = {
  cnameTarget: "<value>",
  mode: "loadBalancer",
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `cnameTarget`                                                                                                      | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | DNS name or URL for the external load balancer.                                                                    |
| `mode`                                                                                                             | [models.SyncAcquireResponseDeploymentModeLoadBalancer](../models/syncacquireresponsedeploymentmodeloadbalancer.md) | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |