# APIKeyDeploymentSetupConfigSourceBuiltIn

## Example Usage

```typescript
import { APIKeyDeploymentSetupConfigSourceBuiltIn } from "@alienplatform/platform-api/models";

let value: APIKeyDeploymentSetupConfigSourceBuiltIn = {
  type: "built-in",
  definitionId: "customer-key",
  version: "<value>",
  sourceReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            | Example                                                                                                |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `type`                                                                                                 | *"built-in"*                                                                                           | :heavy_check_mark:                                                                                     | N/A                                                                                                    |                                                                                                        |
| `definitionId`                                                                                         | [models.APIKeyDeploymentSetupConfigDefinitionId](../models/apikeydeploymentsetupconfigdefinitionid.md) | :heavy_check_mark:                                                                                     | N/A                                                                                                    |                                                                                                        |
| `version`                                                                                              | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |                                                                                                        |
| `sourceReleaseId`                                                                                      | *string*                                                                                               | :heavy_check_mark:                                                                                     | Unique identifier for the release.                                                                     | rel_WbhQgksrawSKIpEN0NAssHX9                                                                           |