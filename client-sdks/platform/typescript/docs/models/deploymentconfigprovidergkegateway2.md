# DeploymentConfigProviderGkeGateway2

## Example Usage

```typescript
import { DeploymentConfigProviderGkeGateway2 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigProviderGkeGateway2 = {
  provider: "gkeGateway",
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `provider`                                                                                             | [models.DeploymentConfigProviderGkeGatewayEnum2](../models/deploymentconfigprovidergkegatewayenum2.md) | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `staticAddressName`                                                                                    | *string*                                                                                               | :heavy_minus_sign:                                                                                     | Optional static address name for the Gateway frontend.                                                 |