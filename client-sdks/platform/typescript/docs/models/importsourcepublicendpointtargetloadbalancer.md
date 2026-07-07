# ImportSourcePublicEndpointTargetLoadBalancer

## Example Usage

```typescript
import { ImportSourcePublicEndpointTargetLoadBalancer } from "@alienplatform/platform-api/models";

let value: ImportSourcePublicEndpointTargetLoadBalancer = {
  cnameTarget: "<value>",
  mode: "loadBalancer",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `cnameTarget`                                                                    | *string*                                                                         | :heavy_check_mark:                                                               | DNS name or URL for the external load balancer.                                  |
| `mode`                                                                           | [models.ImportSourceModeLoadBalancer](../models/importsourcemodeloadbalancer.md) | :heavy_check_mark:                                                               | N/A                                                                              |