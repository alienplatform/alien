# DeploymentProviderGkeGateway2

## Example Usage

```typescript
import { DeploymentProviderGkeGateway2 } from "@alienplatform/platform-api/models";

let value: DeploymentProviderGkeGateway2 = {
  provider: "gkeGateway",
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `provider`                                                                                 | [models.DeploymentProviderGkeGatewayEnum2](../models/deploymentprovidergkegatewayenum2.md) | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `staticAddressName`                                                                        | *string*                                                                                   | :heavy_minus_sign:                                                                         | Optional static address name for the Gateway frontend.                                     |