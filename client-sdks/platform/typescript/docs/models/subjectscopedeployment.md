# SubjectScopeDeployment

## Example Usage

```typescript
import { SubjectScopeDeployment } from "@aliendotdev/platform-api/models";

let value: SubjectScopeDeployment = {
  type: "deployment",
  deploymentId: "<id>",
  projectId: "<id>",
};
```

## Fields

| Field                                               | Type                                                | Required                                            | Description                                         |
| --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- |
| `type`                                              | *"deployment"*                                      | :heavy_check_mark:                                  | Deployment-scoped access                            |
| `deploymentId`                                      | *string*                                            | :heavy_check_mark:                                  | ID of the specific deployment this scope applies to |
| `projectId`                                         | *string*                                            | :heavy_check_mark:                                  | ID of the project this deployment belongs to        |