# ImportSourceProviderAzureApplicationGatewayForContainers2

## Example Usage

```typescript
import { ImportSourceProviderAzureApplicationGatewayForContainers2 } from "@alienplatform/platform-api/models";

let value: ImportSourceProviderAzureApplicationGatewayForContainers2 = {
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
| `provider`                                                                                                                                         | [models.ImportSourceProviderAzureApplicationGatewayForContainersEnum2](../models/importsourceproviderazureapplicationgatewayforcontainersenum2.md) | :heavy_check_mark:                                                                                                                                 | N/A                                                                                                                                                |