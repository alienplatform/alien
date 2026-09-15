# RenderOperatorEcsCloudFormationRequest

## Example Usage

```typescript
import { RenderOperatorEcsCloudFormationRequest } from "@alienplatform/platform-api/models";

let value: RenderOperatorEcsCloudFormationRequest = {
  project: "<value>",
  environmentName: "<value>",
  operatorImagePackageId: "pkg_jebo2o5jmm7raefl2m1pe3cz",
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                | Example                                                                                                                    |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `project`                                                                                                                  | *string*                                                                                                                   | :heavy_check_mark:                                                                                                         | Filter by project ID or name.                                                                                              |                                                                                                                            |
| `environmentName`                                                                                                          | *string*                                                                                                                   | :heavy_check_mark:                                                                                                         | Stable per-environment Remote Operator installation name                                                                   |                                                                                                                            |
| `permission`                                                                                                               | [models.RenderOperatorEcsCloudFormationRequestPermission](../models/renderoperatorecscloudformationrequestpermission.md)   | :heavy_minus_sign:                                                                                                         | Operator permission tier                                                                                                   |                                                                                                                            |
| `operatorImagePackageId`                                                                                                   | *string*                                                                                                                   | :heavy_minus_sign:                                                                                                         | Ready operator-image package to pin in the task definition. If omitted, the current project package is prepared or reused. | pkg_jebo2o5jmm7raefl2m1pe3cz                                                                                               |