# NewDeploymentRequestPublicEndpointTargetLoadBalancer

## Example Usage

```typescript
import { NewDeploymentRequestPublicEndpointTargetLoadBalancer } from "@alienplatform/platform-api/models";

let value: NewDeploymentRequestPublicEndpointTargetLoadBalancer = {
  cnameTarget: "<value>",
  mode: "loadBalancer",
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `cnameTarget`                                                                                    | *string*                                                                                         | :heavy_check_mark:                                                                               | DNS name or URL for the external load balancer.                                                  |
| `mode`                                                                                           | [models.NewDeploymentRequestModeLoadBalancer](../models/newdeploymentrequestmodeloadbalancer.md) | :heavy_check_mark:                                                                               | N/A                                                                                              |