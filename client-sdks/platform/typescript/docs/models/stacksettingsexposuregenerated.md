# StackSettingsExposureGenerated

## Example Usage

```typescript
import { StackSettingsExposureGenerated } from "@alienplatform/platform-api/models";

let value: StackSettingsExposureGenerated = {
  certificate: {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  },
  mode: "generated",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 584209,
    routeApi: "gateway",
  },
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `certificate`                                                                | *models.StackSettingsCertificateUnion1*                                      | :heavy_check_mark:                                                           | Certificate publication or reference mode for Kubernetes public endpoints.   |
| `mode`                                                                       | [models.StackSettingsModeGenerated](../models/stacksettingsmodegenerated.md) | :heavy_check_mark:                                                           | N/A                                                                          |
| `route`                                                                      | *models.StackSettingsRouteUnion1*                                            | :heavy_check_mark:                                                           | Kubernetes route API selected for public endpoints.                          |