# DeploymentSignedSetupUpdate

## Example Usage

```typescript
import { DeploymentSignedSetupUpdate } from "@alienplatform/platform-api/models";

let value: DeploymentSignedSetupUpdate = {
  setupMethod: "terraform",
  fingerprint: "<value>",
  releaseId: "<id>",
  templateUrl: "https://stunning-other.name",
  templateDigest: "<value>",
  installedTemplateDigest: "<value>",
  steps: [],
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `setupMethod`                                                                                        | [models.DeploymentSignedSetupUpdateSetupMethod](../models/deploymentsignedsetupupdatesetupmethod.md) | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `fingerprint`                                                                                        | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `releaseId`                                                                                          | *string*                                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `templateUrl`                                                                                        | *string*                                                                                             | :heavy_check_mark:                                                                                   | The published signed template or archive                                                             |
| `templateDigest`                                                                                     | *string*                                                                                             | :heavy_check_mark:                                                                                   | Digest the release signed for the template                                                           |
| `installedTemplateDigest`                                                                            | *string*                                                                                             | :heavy_check_mark:                                                                                   | Digest of the signed template the install last registered with; null when unknown                    |
| `steps`                                                                                              | [models.DeploymentSignedSetupUpdateStep](../models/deploymentsignedsetupupdatestep.md)[]             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |