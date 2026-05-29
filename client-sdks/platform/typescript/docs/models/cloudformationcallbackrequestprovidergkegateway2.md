# CloudFormationCallbackRequestProviderGkeGateway2

## Example Usage

```typescript
import { CloudFormationCallbackRequestProviderGkeGateway2 } from "@alienplatform/platform-api/models";

let value: CloudFormationCallbackRequestProviderGkeGateway2 = {
  provider: "gkeGateway",
};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `provider`                                                                                                                       | [models.CloudFormationCallbackRequestProviderGkeGatewayEnum2](../models/cloudformationcallbackrequestprovidergkegatewayenum2.md) | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `staticAddressName`                                                                                                              | *string*                                                                                                                         | :heavy_minus_sign:                                                                                                               | Optional static address name for the Gateway frontend.                                                                           |