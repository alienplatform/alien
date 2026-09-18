# ManagerRetryResponsePublicEndpointTargetLoadBalancer3

## Example Usage

```typescript
import { ManagerRetryResponsePublicEndpointTargetLoadBalancer3 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponsePublicEndpointTargetLoadBalancer3 = {
  cnameTarget: "<value>",
  mode: "loadBalancer",
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `cnameTarget`                                                                                      | *string*                                                                                           | :heavy_check_mark:                                                                                 | DNS name or URL for the external load balancer.                                                    |
| `mode`                                                                                             | [models.ManagerRetryResponseModeLoadBalancer3](../models/managerretryresponsemodeloadbalancer3.md) | :heavy_check_mark:                                                                                 | N/A                                                                                                |