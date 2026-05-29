# CloudFormationCallbackRequestExposureGenerated

## Example Usage

```typescript
import { CloudFormationCallbackRequestExposureGenerated } from "@alienplatform/platform-api/models";

let value: CloudFormationCallbackRequestExposureGenerated = {
  certificate: {
    mode: "managedAcmImport",
  },
  mode: "generated",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 348169,
    routeApi: "gateway",
  },
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `certificate`                                                                                                | *models.CloudFormationCallbackRequestCertificateUnion1*                                                      | :heavy_check_mark:                                                                                           | Certificate publication or reference mode for Kubernetes public endpoints.                                   |
| `mode`                                                                                                       | [models.CloudFormationCallbackRequestModeGenerated](../models/cloudformationcallbackrequestmodegenerated.md) | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `route`                                                                                                      | *models.CloudFormationCallbackRequestRouteUnion1*                                                            | :heavy_check_mark:                                                                                           | Kubernetes route API selected for public endpoints.                                                          |