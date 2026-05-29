# ProviderAzureApplicationGatewayForContainers2

## Example Usage

```typescript
import { ProviderAzureApplicationGatewayForContainers2 } from "@alienplatform/platform-api/models/operations";

let value: ProviderAzureApplicationGatewayForContainers2 = {
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
| `provider`                                                                                                                                   | [operations.ProviderAzureApplicationGatewayForContainersEnum2](../../models/operations/providerazureapplicationgatewayforcontainersenum2.md) | :heavy_check_mark:                                                                                                                           | N/A                                                                                                                                          |