# RouteGateway2

Shared Gateway API route profile values.

## Example Usage

```typescript
import { RouteGateway2 } from "@alienplatform/platform-api/models/operations";

let value: RouteGateway2 = {
  gatewayClassName: "<value>",
  listenerPort: 271202,
  routeApi: "gateway",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `annotations`                                                        | Record<string, *string*>                                             | :heavy_minus_sign:                                                   | Annotations applied to route objects.                                |
| `controller`                                                         | *string*                                                             | :heavy_minus_sign:                                                   | Route controller identifier, for example a cloud Gateway controller. |
| `gatewayClassName`                                                   | *string*                                                             | :heavy_check_mark:                                                   | GatewayClass selected for generated Gateways.                        |
| `labels`                                                             | Record<string, *string*>                                             | :heavy_minus_sign:                                                   | Labels applied to route objects.                                     |
| `listenerPort`                                                       | *number*                                                             | :heavy_check_mark:                                                   | Listener port, usually 443.                                          |
| `provider`                                                           | *operations.ProviderUnion4*                                          | :heavy_minus_sign:                                                   | N/A                                                                  |
| `routeApi`                                                           | *"gateway"*                                                          | :heavy_check_mark:                                                   | N/A                                                                  |