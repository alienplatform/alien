# CreateManagerResponsePublicEndpointTargetLoadBalancer2

## Example Usage

```typescript
import { CreateManagerResponsePublicEndpointTargetLoadBalancer2 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponsePublicEndpointTargetLoadBalancer2 = {
  cnameTarget: "<value>",
  mode: "loadBalancer",
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `cnameTarget`                                                                                        | *string*                                                                                             | :heavy_check_mark:                                                                                   | DNS name or URL for the external load balancer.                                                      |
| `mode`                                                                                               | [models.CreateManagerResponseModeLoadBalancer2](../models/createmanagerresponsemodeloadbalancer2.md) | :heavy_check_mark:                                                                                   | N/A                                                                                                  |