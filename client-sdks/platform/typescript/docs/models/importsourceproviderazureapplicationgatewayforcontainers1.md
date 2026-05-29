# ImportSourceProviderAzureApplicationGatewayForContainers1

## Example Usage

```typescript
import { ImportSourceProviderAzureApplicationGatewayForContainers1 } from "@alienplatform/platform-api/models";

let value: ImportSourceProviderAzureApplicationGatewayForContainers1 = {
  frontend: "<value>",
  provider: "azureApplicationGatewayForContainers",
};
```

## Fields

| Field                                                                                                                                              | Type                                                                                                                                               | Required                                                                                                                                           | Description                                                                                                                                        |
| -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| `albName`                                                                                                                                          | *string*                                                                                                                                           | :heavy_minus_sign:                                                                                                                                 | Optional ALB name when using BYO Application Gateway resources.                                                                                    |
| `albNamespace`                                                                                                                                     | *string*                                                                                                                                           | :heavy_minus_sign:                                                                                                                                 | Optional ALB namespace when using BYO Application Gateway resources.                                                                               |
| `frontend`                                                                                                                                         | *string*                                                                                                                                           | :heavy_check_mark:                                                                                                                                 | Public or internal frontend exposure.                                                                                                              |
| `provider`                                                                                                                                         | [models.ImportSourceProviderAzureApplicationGatewayForContainersEnum1](../models/importsourceproviderazureapplicationgatewayforcontainersenum1.md) | :heavy_check_mark:                                                                                                                                 | N/A                                                                                                                                                |