# PrepareDeploymentStackPublicEndpointTargetLoadBalancer

## Example Usage

```typescript
import { PrepareDeploymentStackPublicEndpointTargetLoadBalancer } from "@alienplatform/platform-api/models/operations";

let value: PrepareDeploymentStackPublicEndpointTargetLoadBalancer = {
  cnameTarget: "<value>",
  mode: "loadBalancer",
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `cnameTarget`                                                                                                          | *string*                                                                                                               | :heavy_check_mark:                                                                                                     | DNS name or URL for the external load balancer.                                                                        |
| `mode`                                                                                                                 | [operations.PrepareDeploymentStackModeLoadBalancer](../../models/operations/preparedeploymentstackmodeloadbalancer.md) | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |