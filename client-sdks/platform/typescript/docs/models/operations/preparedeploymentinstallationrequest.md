# PrepareDeploymentInstallationRequest

## Example Usage

```typescript
import { PrepareDeploymentInstallationRequest } from "@alienplatform/platform-api/models/operations";

let value: PrepareDeploymentInstallationRequest = {
  reference: "29970d31-559f-4512-ac3a-e832a339aa21",
  requestBody: {
    name: "<value>",
    platform: "aws",
    method: "cli",
    configuration: "<value>",
    command: "<value>",
  },
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `reference`                                                                                                                | *string*                                                                                                                   | :heavy_check_mark:                                                                                                         | N/A                                                                                                                        |
| `requestBody`                                                                                                              | [operations.PrepareDeploymentInstallationRequestBody](../../models/operations/preparedeploymentinstallationrequestbody.md) | :heavy_check_mark:                                                                                                         | N/A                                                                                                                        |