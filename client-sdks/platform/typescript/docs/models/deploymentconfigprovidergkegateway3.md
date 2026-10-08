# DeploymentConfigProviderGkeGateway3

## Example Usage

```typescript
import { DeploymentConfigProviderGkeGateway3 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigProviderGkeGateway3 = {
  provider: "gkeGateway",
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `provider`                                                                                             | [models.DeploymentConfigProviderGkeGatewayEnum3](../models/deploymentconfigprovidergkegatewayenum3.md) | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `staticAddressName`                                                                                    | *string*                                                                                               | :heavy_minus_sign:                                                                                     | Optional static address name for the Gateway frontend.                                                 |