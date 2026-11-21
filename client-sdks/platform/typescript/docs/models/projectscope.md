# ProjectScope

Project-scoped configuration

## Example Usage

```typescript
import { ProjectScope } from "@alienplatform/platform-api/models";

let value: ProjectScope = {
  type: "project",
  projectId: "<id>",
  role: "project.viewer",
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       | Example                                                                                                                           |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `type`                                                                                                                            | *"project"*                                                                                                                       | :heavy_check_mark:                                                                                                                | N/A                                                                                                                               |                                                                                                                                   |
| `projectId`                                                                                                                       | *string*                                                                                                                          | :heavy_check_mark:                                                                                                                | ID of the project this is scoped to                                                                                               |                                                                                                                                   |
| `role`                                                                                                                            | [models.ProjectRole](../models/projectrole.md)                                                                                    | :heavy_check_mark:                                                                                                                | Role for project-scoped service accounts                                                                                          | workspace.member                                                                                                                  |
| `additionalRoles`                                                                                                                 | [models.ApplicationRole](../models/applicationrole.md)[]                                                                          | :heavy_minus_sign:                                                                                                                | More application roles the key holds beside `role`, which must be an application role too. One key then reaches several features. |                                                                                                                                   |