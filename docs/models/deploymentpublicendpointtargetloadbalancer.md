# DeploymentPublicEndpointTargetLoadBalancer

## Example Usage

```typescript
import { DeploymentPublicEndpointTargetLoadBalancer } from "@alienplatform/platform-api/models";

let value: DeploymentPublicEndpointTargetLoadBalancer = {
  cnameTarget: "<value>",
  mode: "loadBalancer",
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `cnameTarget`                                                                | *string*                                                                     | :heavy_check_mark:                                                           | DNS name or URL for the external load balancer.                              |
| `mode`                                                                       | [models.DeploymentModeLoadBalancer](../models/deploymentmodeloadbalancer.md) | :heavy_check_mark:                                                           | N/A                                                                          |