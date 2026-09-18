# CreateManagerResponsePublicEndpointTargetLoadBalancer3

## Example Usage

```typescript
import { CreateManagerResponsePublicEndpointTargetLoadBalancer3 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponsePublicEndpointTargetLoadBalancer3 = {
  cnameTarget: "<value>",
  mode: "loadBalancer",
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `cnameTarget`                                                                                        | *string*                                                                                             | :heavy_check_mark:                                                                                   | DNS name or URL for the external load balancer.                                                      |
| `mode`                                                                                               | [models.CreateManagerResponseModeLoadBalancer3](../models/createmanagerresponsemodeloadbalancer3.md) | :heavy_check_mark:                                                                                   | N/A                                                                                                  |