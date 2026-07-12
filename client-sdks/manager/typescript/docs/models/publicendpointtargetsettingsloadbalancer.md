# PublicEndpointTargetSettingsLoadBalancer

Publish a CNAME record to an external load balancer.

## Example Usage

```typescript
import { PublicEndpointTargetSettingsLoadBalancer } from "@alienplatform/manager-api/models";

let value: PublicEndpointTargetSettingsLoadBalancer = {
  cnameTarget: "<value>",
  mode: "loadBalancer",
};
```

## Fields

| Field                                           | Type                                            | Required                                        | Description                                     |
| ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- |
| `cnameTarget`                                   | *string*                                        | :heavy_check_mark:                              | DNS name or URL for the external load balancer. |
| `mode`                                          | *"loadBalancer"*                                | :heavy_check_mark:                              | N/A                                             |