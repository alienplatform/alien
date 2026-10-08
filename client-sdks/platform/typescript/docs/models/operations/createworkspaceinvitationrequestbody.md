# CreateWorkspaceInvitationRequestBody

## Example Usage

```typescript
import { CreateWorkspaceInvitationRequestBody } from "@alienplatform/platform-api/models/operations";

let value: CreateWorkspaceInvitationRequestBody = {
  email: "Tyrell_Kris74@hotmail.com",
  role: "workspace.member",
};
```

## Fields

| Field                                                 | Type                                                  | Required                                              | Description                                           | Example                                               |
| ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- |
| `email`                                               | *string*                                              | :heavy_check_mark:                                    | N/A                                                   |                                                       |
| `role`                                                | [models.WorkspaceRole](../../models/workspacerole.md) | :heavy_check_mark:                                    | Role for workspace-scoped service accounts            | workspace.member                                      |
