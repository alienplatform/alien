# CreateSetupRegistrationOperationRequest

## Example Usage

```typescript
import { CreateSetupRegistrationOperationRequest } from "@alienplatform/platform-api/models";

let value: CreateSetupRegistrationOperationRequest = {
  action: "create",
  sourceKind: "helm",
  source: {
    deploymentName: "<value>",
    resourcePrefix: "<value>",
    releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    platform: "local",
    region: "<value>",
    setupTarget: "<value>",
    setupImportFormatVersion: 339567,
    setupFingerprint: "<value>",
    setupFingerprintVersion: 507566,
    stackSettings: {},
    resources: [
      {
        id: "<id>",
        type: "<value>",
        importData: {},
      },
    ],
  },
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        | Example                                                                                                            |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `action`                                                                                                           | [models.SetupRegistrationAction](../models/setupregistrationaction.md)                                             | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |                                                                                                                    |
| `sourceKind`                                                                                                       | [models.ImportSourceKind](../models/importsourcekind.md)                                                           | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |                                                                                                                    |
| `source`                                                                                                           | [models.CreateSetupRegistrationOperationRequestSource](../models/createsetupregistrationoperationrequestsource.md) | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |                                                                                                                    |
| `inputValues`                                                                                                      | Record<string, *models.StackInputValueRequest*>                                                                    | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |                                                                                                                    |
| `deploymentId`                                                                                                     | *string*                                                                                                           | :heavy_minus_sign:                                                                                                 | Unique identifier for the deployment.                                                                              | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                                       |
| `idempotencyKey`                                                                                                   | *string*                                                                                                           | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |                                                                                                                    |
| `cloudFormation`                                                                                                   | [models.SetupRegistrationCloudFormationTarget](../models/setupregistrationcloudformationtarget.md)                 | :heavy_minus_sign:                                                                                                 | N/A                                                                                                                |                                                                                                                    |