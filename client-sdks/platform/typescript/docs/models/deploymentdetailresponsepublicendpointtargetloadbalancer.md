# DeploymentDetailResponsePublicEndpointTargetLoadBalancer

## Example Usage

```typescript
import { DeploymentDetailResponsePublicEndpointTargetLoadBalancer } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePublicEndpointTargetLoadBalancer = {
  cnameTarget: "<value>",
  mode: "loadBalancer",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `cnameTarget`                                                                                            | *string*                                                                                                 | :heavy_check_mark:                                                                                       | DNS name or URL for the external load balancer.                                                          |
| `mode`                                                                                                   | [models.DeploymentDetailResponseModeLoadBalancer](../models/deploymentdetailresponsemodeloadbalancer.md) | :heavy_check_mark:                                                                                       | N/A                                                                                                      |