# TargetDeploymentProviderGkeGateway2

## Example Usage

```typescript
import { TargetDeploymentProviderGkeGateway2 } from "@alienplatform/platform-api/models";

let value: TargetDeploymentProviderGkeGateway2 = {
  provider: "gkeGateway",
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `provider`                                                                                             | [models.TargetDeploymentProviderGkeGatewayEnum2](../models/targetdeploymentprovidergkegatewayenum2.md) | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `staticAddressName`                                                                                    | *string*                                                                                               | :heavy_minus_sign:                                                                                     | Optional static address name for the Gateway frontend.                                                 |