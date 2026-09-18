# SetupItemStatusSourceBuiltIn

## Example Usage

```typescript
import { SetupItemStatusSourceBuiltIn } from "@alienplatform/platform-api/models";

let value: SetupItemStatusSourceBuiltIn = {
  type: "built-in",
  definitionId: "customer-ai",
  version: "<value>",
  sourceReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    | Example                                                                        |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `type`                                                                         | *"built-in"*                                                                   | :heavy_check_mark:                                                             | N/A                                                                            |                                                                                |
| `definitionId`                                                                 | [models.SetupItemStatusDefinitionId](../models/setupitemstatusdefinitionid.md) | :heavy_check_mark:                                                             | N/A                                                                            |                                                                                |
| `version`                                                                      | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |                                                                                |
| `sourceReleaseId`                                                              | *string*                                                                       | :heavy_check_mark:                                                             | Unique identifier for the release.                                             | rel_WbhQgksrawSKIpEN0NAssHX9                                                   |