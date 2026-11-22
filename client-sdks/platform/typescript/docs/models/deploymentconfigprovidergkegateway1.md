# DeploymentConfigProviderGkeGateway1

## Example Usage

```typescript
import { DeploymentConfigProviderGkeGateway1 } from "@alienplatform/platform-api/models";

let value: DeploymentConfigProviderGkeGateway1 = {
  provider: "gkeGateway",
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `provider`                                                                                             | [models.DeploymentConfigProviderGkeGatewayEnum1](../models/deploymentconfigprovidergkegatewayenum1.md) | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `staticAddressName`                                                                                    | *string*                                                                                               | :heavy_minus_sign:                                                                                     | Optional static address name for the Gateway frontend.                                                 |