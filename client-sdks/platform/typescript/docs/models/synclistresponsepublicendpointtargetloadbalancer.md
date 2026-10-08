# SyncListResponsePublicEndpointTargetLoadBalancer

## Example Usage

```typescript
import { SyncListResponsePublicEndpointTargetLoadBalancer } from "@alienplatform/platform-api/models";

let value: SyncListResponsePublicEndpointTargetLoadBalancer = {
  cnameTarget: "<value>",
  mode: "loadBalancer",
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `cnameTarget`                                                                            | *string*                                                                                 | :heavy_check_mark:                                                                       | DNS name or URL for the external load balancer.                                          |
| `mode`                                                                                   | [models.SyncListResponseModeLoadBalancer](../models/synclistresponsemodeloadbalancer.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |