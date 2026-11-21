# DeploymentConfigProviderGkeGateway4

## Example Usage

```typescript
import { DeploymentConfigProviderGkeGateway4 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigProviderGkeGateway4 = {
  provider: "gkeGateway",
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `provider`                                                                                             | [models.DeploymentConfigProviderGkeGatewayEnum4](../models/deploymentconfigprovidergkegatewayenum4.md) | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `staticAddressName`                                                                                    | *string*                                                                                               | :heavy_minus_sign:                                                                                     | Optional static address name for the Gateway frontend.                                                 |