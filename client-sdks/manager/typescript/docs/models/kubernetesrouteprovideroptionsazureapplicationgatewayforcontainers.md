# KubernetesRouteProviderOptionsAzureApplicationGatewayForContainers

Azure Application Gateway for Containers route options.

## Example Usage

```typescript
import { KubernetesRouteProviderOptionsAzureApplicationGatewayForContainers } from "@alienplatform/manager-api/models";

let value: KubernetesRouteProviderOptionsAzureApplicationGatewayForContainers =
  {
    frontend: "<value>",
    provider: "azureApplicationGatewayForContainers",
  };
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `albName`                                                            | *string*                                                             | :heavy_minus_sign:                                                   | Optional ALB name when using BYO Application Gateway resources.      |
| `albNamespace`                                                       | *string*                                                             | :heavy_minus_sign:                                                   | Optional ALB namespace when using BYO Application Gateway resources. |
| `frontend`                                                           | *string*                                                             | :heavy_check_mark:                                                   | Public or internal frontend exposure.                                |
| `provider`                                                           | *"azureApplicationGatewayForContainers"*                             | :heavy_check_mark:                                                   | N/A                                                                  |