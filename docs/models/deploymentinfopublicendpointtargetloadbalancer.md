# DeploymentInfoPublicEndpointTargetLoadBalancer

## Example Usage

```typescript
import { DeploymentInfoPublicEndpointTargetLoadBalancer } from "@alienplatform/platform-api/models";

let value: DeploymentInfoPublicEndpointTargetLoadBalancer = {
  cnameTarget: "<value>",
  mode: "loadBalancer",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `cnameTarget`                                                                  | *string*                                                                       | :heavy_check_mark:                                                             | DNS name or URL for the external load balancer.                                |
| `mode`                                                                         | [models.SetupUpdateModeLoadBalancer](../models/setupupdatemodeloadbalancer.md) | :heavy_check_mark:                                                             | N/A                                                                            |