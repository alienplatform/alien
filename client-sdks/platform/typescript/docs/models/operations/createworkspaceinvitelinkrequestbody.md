# CreateWorkspaceInviteLinkRequestBody

## Example Usage

```typescript
import { CreateWorkspaceInviteLinkRequestBody } from "@alienplatform/platform-api/models/operations";

let value: CreateWorkspaceInviteLinkRequestBody = {
  role: "workspace.member",
};
```

## Fields

| Field                                                 | Type                                                  | Required                                              | Description                                           | Example                                               |
| ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- |
| `role`                                                | [models.WorkspaceRole](../../models/workspacerole.md) | :heavy_check_mark:                                    | Role for workspace-scoped service accounts            | workspace.member                                      |
