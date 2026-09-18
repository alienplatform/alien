# CreateManagerResponsePublicEndpointTargetLoadBalancer1

## Example Usage

```typescript
import { CreateManagerResponsePublicEndpointTargetLoadBalancer1 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponsePublicEndpointTargetLoadBalancer1 = {
  cnameTarget: "<value>",
  mode: "loadBalancer",
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `cnameTarget`                                                                                        | *string*                                                                                             | :heavy_check_mark:                                                                                   | DNS name or URL for the external load balancer.                                                      |
| `mode`                                                                                               | [models.CreateManagerResponseModeLoadBalancer1](../models/createmanagerresponsemodeloadbalancer1.md) | :heavy_check_mark:                                                                                   | N/A                                                                                                  |