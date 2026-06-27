# PrepareDeploymentStackRouteIngress1

Shared Ingress route profile values.

## Example Usage

```typescript
import { PrepareDeploymentStackRouteIngress1 } from "@alienplatform/platform-api/models/operations";

let value: PrepareDeploymentStackRouteIngress1 = {
  ingressClassName: "<value>",
  routeApi: "ingress",
};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `annotations`                                                     | Record<string, *string*>                                          | :heavy_minus_sign:                                                | Annotations applied to route objects.                             |
| `controller`                                                      | *string*                                                          | :heavy_minus_sign:                                                | Route controller identifier, for example `eks.amazonaws.com/alb`. |
| `ingressClassName`                                                | *string*                                                          | :heavy_check_mark:                                                | `spec.ingressClassName` for generated Ingresses.                  |
| `labels`                                                          | Record<string, *string*>                                          | :heavy_minus_sign:                                                | Labels applied to route objects.                                  |
| `provider`                                                        | *operations.PrepareDeploymentStackProviderUnion1*                 | :heavy_minus_sign:                                                | N/A                                                               |
| `routeApi`                                                        | *"ingress"*                                                       | :heavy_check_mark:                                                | N/A                                                               |