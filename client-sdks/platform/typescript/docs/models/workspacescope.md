# WorkspaceScope

Workspace-scoped configuration

## Example Usage

```typescript
import { WorkspaceScope } from "@alienplatform/platform-api/models";

let value: WorkspaceScope = {
  type: "workspace",
  role: "workspace.member",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        | Example                                            |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `type`                                             | *"workspace"*                                      | :heavy_check_mark:                                 | N/A                                                |                                                    |
| `role`                                             | [models.WorkspaceRole](../models/workspacerole.md) | :heavy_check_mark:                                 | Role for workspace-scoped service accounts         | workspace.member                                   |