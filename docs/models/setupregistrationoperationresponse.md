# SetupRegistrationOperationResponse

## Example Usage

```typescript
import { SetupRegistrationOperationResponse } from "@alienplatform/platform-api/models";

let value: SetupRegistrationOperationResponse = {
  id: "setupop_y41lqnfosxuwqkzmiax7",
  action: "delete",
  sourceKind: "helm",
  status: "succeeded",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  physicalResourceId: "<id>",
  result: {
    deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
    deploymentToken: "<value>",
    helmValues: "<value>",
  },
  error: {
    message: "<value>",
    retryable: false,
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            | Example                                                                                                |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `id`                                                                                                   | *string*                                                                                               | :heavy_check_mark:                                                                                     | Unique identifier for the setup registration operation.                                                | setupop_y41lqnfosxuwqkzmiax7                                                                           |
| `action`                                                                                               | [models.SetupRegistrationAction](../models/setupregistrationaction.md)                                 | :heavy_check_mark:                                                                                     | N/A                                                                                                    |                                                                                                        |
| `sourceKind`                                                                                           | [models.ImportSourceKind](../models/importsourcekind.md)                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |                                                                                                        |
| `status`                                                                                               | [models.SetupRegistrationOperationStatus](../models/setupregistrationoperationstatus.md)               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |                                                                                                        |
| `deploymentId`                                                                                         | *string*                                                                                               | :heavy_check_mark:                                                                                     | Unique identifier for the deployment.                                                                  | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                           |
| `physicalResourceId`                                                                                   | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |                                                                                                        |
| `result`                                                                                               | [models.SetupRegistrationOperationResult](../models/setupregistrationoperationresult.md)               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |                                                                                                        |
| `error`                                                                                                | [models.SetupRegistrationOperationResponseError](../models/setupregistrationoperationresponseerror.md) | :heavy_check_mark:                                                                                     | N/A                                                                                                    |                                                                                                        |