# ProviderAzureApplicationGatewayForContainers4

## Example Usage

```typescript
import { ProviderAzureApplicationGatewayForContainers4 } from "@alienplatform/platform-api/models/operations";

let value: ProviderAzureApplicationGatewayForContainers4 = {
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
| `provider`                                                                                                                                   | [operations.ProviderAzureApplicationGatewayForContainersEnum4](../../models/operations/providerazureapplicationgatewayforcontainersenum4.md) | :heavy_check_mark:                                                                                                                           | N/A                                                                                                                                          |