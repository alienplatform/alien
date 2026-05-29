# KubernetesRouteProviderOptionsGkeGateway

GKE Gateway route options.

## Example Usage

```typescript
import { KubernetesRouteProviderOptionsGkeGateway } from "@alienplatform/manager-api/models";

let value: KubernetesRouteProviderOptionsGkeGateway = {
  provider: "gkeGateway",
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `provider`                                             | *"gkeGateway"*                                         | :heavy_check_mark:                                     | N/A                                                    |
| `staticAddressName`                                    | *string*                                               | :heavy_minus_sign:                                     | Optional static address name for the Gateway frontend. |