# KubernetesExposureCustomProviderGkeGateway2

## Example Usage

```typescript
import { KubernetesExposureCustomProviderGkeGateway2 } from "@alienplatform/platform-api/models";

let value: KubernetesExposureCustomProviderGkeGateway2 = {
  provider: "gkeGateway",
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `provider`                                                                                                             | [models.KubernetesExposureCustomProviderGkeGatewayEnum2](../models/kubernetesexposurecustomprovidergkegatewayenum2.md) | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `staticAddressName`                                                                                                    | *string*                                                                                                               | :heavy_minus_sign:                                                                                                     | Optional static address name for the Gateway frontend.                                                                 |