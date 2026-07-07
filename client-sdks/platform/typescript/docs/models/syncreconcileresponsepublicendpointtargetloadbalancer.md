# SyncReconcileResponsePublicEndpointTargetLoadBalancer

## Example Usage

```typescript
import { SyncReconcileResponsePublicEndpointTargetLoadBalancer } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePublicEndpointTargetLoadBalancer = {
  cnameTarget: "<value>",
  mode: "loadBalancer",
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `cnameTarget`                                                                                      | *string*                                                                                           | :heavy_check_mark:                                                                                 | DNS name or URL for the external load balancer.                                                    |
| `mode`                                                                                             | [models.SyncReconcileResponseModeLoadBalancer](../models/syncreconcileresponsemodeloadbalancer.md) | :heavy_check_mark:                                                                                 | N/A                                                                                                |