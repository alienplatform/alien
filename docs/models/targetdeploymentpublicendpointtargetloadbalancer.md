# TargetDeploymentPublicEndpointTargetLoadBalancer

## Example Usage

```typescript
import { TargetDeploymentPublicEndpointTargetLoadBalancer } from "@alienplatform/platform-api/models";

let value: TargetDeploymentPublicEndpointTargetLoadBalancer = {
  cnameTarget: "<value>",
  mode: "loadBalancer",
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `cnameTarget`                                                                            | *string*                                                                                 | :heavy_check_mark:                                                                       | DNS name or URL for the external load balancer.                                          |
| `mode`                                                                                   | [models.TargetDeploymentModeLoadBalancer](../models/targetdeploymentmodeloadbalancer.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |