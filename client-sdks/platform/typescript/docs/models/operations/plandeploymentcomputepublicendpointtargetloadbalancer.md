# PlanDeploymentComputePublicEndpointTargetLoadBalancer

## Example Usage

```typescript
import { PlanDeploymentComputePublicEndpointTargetLoadBalancer } from "@alienplatform/platform-api/models/operations";

let value: PlanDeploymentComputePublicEndpointTargetLoadBalancer = {
  cnameTarget: "<value>",
  mode: "loadBalancer",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `cnameTarget`                                                                                                        | *string*                                                                                                             | :heavy_check_mark:                                                                                                   | DNS name or URL for the external load balancer.                                                                      |
| `mode`                                                                                                               | [operations.PlanDeploymentComputeModeLoadBalancer](../../models/operations/plandeploymentcomputemodeloadbalancer.md) | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |