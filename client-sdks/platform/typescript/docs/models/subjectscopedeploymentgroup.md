# SubjectScopeDeploymentGroup

## Example Usage

```typescript
import { SubjectScopeDeploymentGroup } from "@alienplatform/platform-api/models";

let value: SubjectScopeDeploymentGroup = {
  type: "deployment-group",
  deploymentGroupId: "<id>",
  projectId: "<id>",
};
```

## Fields

| Field                                                     | Type                                                      | Required                                                  | Description                                               |
| --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- |
| `type`                                                    | *"deployment-group"*                                      | :heavy_check_mark:                                        | Deployment group-scoped access                            |
| `deploymentGroupId`                                       | *string*                                                  | :heavy_check_mark:                                        | ID of the specific deployment group this scope applies to |
| `projectId`                                               | *string*                                                  | :heavy_check_mark:                                        | ID of the project this deployment group belongs to        |