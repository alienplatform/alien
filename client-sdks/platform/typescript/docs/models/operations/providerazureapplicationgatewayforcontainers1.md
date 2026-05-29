# ProviderAzureApplicationGatewayForContainers1

## Example Usage

```typescript
import { ProviderAzureApplicationGatewayForContainers1 } from "@alienplatform/platform-api/models/operations";

let value: ProviderAzureApplicationGatewayForContainers1 = {
  frontend: "<value>",
  provider: "azureApplicationGatewayForContainers",
};
```

## Fields

| Field                                                                                                                                        | Type                                                                                                                                         | Required                                                                                                                                     | Description                                                                                                                                  |
| -------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| `albName`                                                                                                                                    | *string*                                                                                                                                     | :heavy_minus_sign:                                                                                                                           | Optional ALB name when using BYO Application Gateway resources.                                                                              |
| `albNamespace`                                                                                                                               | *string*                                                                                                                                     | :heavy_minus_sign:                                                                                                                           | Optional ALB namespace when using BYO Application Gateway resources.                                                                         |
| `frontend`                                                                                                                                   | *string*                                                                                                                                     | :heavy_check_mark:                                                                                                                           | Public or internal frontend exposure.                                                                                                        |
| `provider`                                                                                                                                   | [operations.ProviderAzureApplicationGatewayForContainersEnum1](../../models/operations/providerazureapplicationgatewayforcontainersenum1.md) | :heavy_check_mark:                                                                                                                           | N/A                                                                                                                                          |