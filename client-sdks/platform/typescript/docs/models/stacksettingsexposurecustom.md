# StackSettingsExposureCustom

## Example Usage

```typescript
import { StackSettingsExposureCustom } from "@alienplatform/platform-api/models";

let value: StackSettingsExposureCustom = {
  certificate: {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  },
  domain: "gracious-compromise.info",
  mode: "custom",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 122317,
    routeApi: "gateway",
  },
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `certificate`                                                              | *models.StackSettingsCertificateUnion2*                                    | :heavy_check_mark:                                                         | Certificate publication or reference mode for Kubernetes public endpoints. |
| `domain`                                                                   | *string*                                                                   | :heavy_check_mark:                                                         | Hostname routed by the Kubernetes public endpoint.                         |
| `mode`                                                                     | [models.StackSettingsModeCustom](../models/stacksettingsmodecustom.md)     | :heavy_check_mark:                                                         | N/A                                                                        |
| `route`                                                                    | *models.StackSettingsRouteUnion2*                                          | :heavy_check_mark:                                                         | Kubernetes route API selected for public endpoints.                        |