# DeploymentConfigPublicEndpointTargetLoadBalancer

## Example Usage

```typescript
import { DeploymentConfigPublicEndpointTargetLoadBalancer } from "@alienplatform/platform-api/models";

let value: DeploymentConfigPublicEndpointTargetLoadBalancer = {
  cnameTarget: "<value>",
  mode: "loadBalancer",
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `cnameTarget`                                                                            | *string*                                                                                 | :heavy_check_mark:                                                                       | DNS name or URL for the external load balancer.                                          |
| `mode`                                                                                   | [models.DeploymentConfigModeLoadBalancer](../models/deploymentconfigmodeloadbalancer.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |