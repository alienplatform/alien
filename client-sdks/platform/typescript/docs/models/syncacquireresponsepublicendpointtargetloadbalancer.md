# SyncAcquireResponsePublicEndpointTargetLoadBalancer

## Example Usage

```typescript
import { SyncAcquireResponsePublicEndpointTargetLoadBalancer } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponsePublicEndpointTargetLoadBalancer = {
  cnameTarget: "<value>",
  mode: "loadBalancer",
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `cnameTarget`                                                                                  | *string*                                                                                       | :heavy_check_mark:                                                                             | DNS name or URL for the external load balancer.                                                |
| `mode`                                                                                         | [models.SyncAcquireResponseModeLoadBalancer](../models/syncacquireresponsemodeloadbalancer.md) | :heavy_check_mark:                                                                             | N/A                                                                                            |