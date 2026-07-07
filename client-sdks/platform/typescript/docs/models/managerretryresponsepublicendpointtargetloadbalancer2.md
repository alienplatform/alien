# ManagerRetryResponsePublicEndpointTargetLoadBalancer2

## Example Usage

```typescript
import { ManagerRetryResponsePublicEndpointTargetLoadBalancer2 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponsePublicEndpointTargetLoadBalancer2 = {
  cnameTarget: "<value>",
  mode: "loadBalancer",
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `cnameTarget`                                                                                      | *string*                                                                                           | :heavy_check_mark:                                                                                 | DNS name or URL for the external load balancer.                                                    |
| `mode`                                                                                             | [models.ManagerRetryResponseModeLoadBalancer2](../models/managerretryresponsemodeloadbalancer2.md) | :heavy_check_mark:                                                                                 | N/A                                                                                                |