# DeploymentProviderAzureApplicationGatewayForContainers2

## Example Usage

```typescript
import { DeploymentProviderAzureApplicationGatewayForContainers2 } from "@alienplatform/platform-api/models";

let value: DeploymentProviderAzureApplicationGatewayForContainers2 = {
  frontend: "<value>",
  provider: "azureApplicationGatewayForContainers",
};
```

## Fields

| Field                                                                                                                                          | Type                                                                                                                                           | Required                                                                                                                                       | Description                                                                                                                                    |
| ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `albName`                                                                                                                                      | *string*                                                                                                                                       | :heavy_minus_sign:                                                                                                                             | Optional ALB name when using BYO Application Gateway resources.                                                                                |
| `albNamespace`                                                                                                                                 | *string*                                                                                                                                       | :heavy_minus_sign:                                                                                                                             | Optional ALB namespace when using BYO Application Gateway resources.                                                                           |
| `frontend`                                                                                                                                     | *string*                                                                                                                                       | :heavy_check_mark:                                                                                                                             | Public or internal frontend exposure.                                                                                                          |
| `provider`                                                                                                                                     | [models.DeploymentProviderAzureApplicationGatewayForContainersEnum2](../models/deploymentproviderazureapplicationgatewayforcontainersenum2.md) | :heavy_check_mark:                                                                                                                             | N/A                                                                                                                                            |