# ManagerRetryResponsePublicEndpointTargetLoadBalancer1

## Example Usage

```typescript
import { ManagerRetryResponsePublicEndpointTargetLoadBalancer1 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponsePublicEndpointTargetLoadBalancer1 = {
  cnameTarget: "<value>",
  mode: "loadBalancer",
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `cnameTarget`                                                                                      | *string*                                                                                           | :heavy_check_mark:                                                                                 | DNS name or URL for the external load balancer.                                                    |
| `mode`                                                                                             | [models.ManagerRetryResponseModeLoadBalancer1](../models/managerretryresponsemodeloadbalancer1.md) | :heavy_check_mark:                                                                                 | N/A                                                                                                |