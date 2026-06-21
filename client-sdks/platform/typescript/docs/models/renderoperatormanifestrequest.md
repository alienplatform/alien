# RenderOperatorManifestRequest

## Example Usage

```typescript
import { RenderOperatorManifestRequest } from "@alienplatform/platform-api/models";

let value: RenderOperatorManifestRequest = {
  project: "my-project",
  name: "my-app",
  namespace: "<value>",
  deploymentGroupToken: "<value>",
};
```

## Fields

| Field                                                     | Type                                                      | Required                                                  | Description                                               | Example                                                   |
| --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- |
| `project`                                                 | *string*                                                  | :heavy_check_mark:                                        | Filter by project ID or name.                             | my-project                                                |
| `name`                                                    | *string*                                                  | :heavy_check_mark:                                        | Environment name used as the operator identity            | my-app                                                    |
| `namespace`                                               | *string*                                                  | :heavy_check_mark:                                        | Kubernetes namespace to install into and observe          |                                                           |
| `scope`                                                   | *string*                                                  | :heavy_minus_sign:                                        | Namespace scope to observe. Defaults to namespace.        |                                                           |
| `permission`                                              | [models.Permission](../models/permission.md)              | :heavy_minus_sign:                                        | Operator permission tier                                  |                                                           |
| `deploymentGroupToken`                                    | *string*                                                  | :heavy_check_mark:                                        | Deployment-group token embedded in the operator Secret    |                                                           |
| `logCollector`                                            | [models.LogCollector](../models/logcollector.md)          | :heavy_minus_sign:                                        | Enable the node log collector DaemonSet for raw pod logs. |                                                           |