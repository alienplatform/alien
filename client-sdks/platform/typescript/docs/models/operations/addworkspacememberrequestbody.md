# AddWorkspaceMemberRequestBody

## Example Usage

```typescript
import { AddWorkspaceMemberRequestBody } from "@aliendotdev/platform-api/models/operations";

let value: AddWorkspaceMemberRequestBody = {
  email: "Marlon.Ebert55@yahoo.com",
  role: "workspace.member",
};
```

## Fields

| Field                                                 | Type                                                  | Required                                              | Description                                           | Example                                               |
| ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- |
| `email`                                               | *string*                                              | :heavy_check_mark:                                    | Email of the user to add                              |                                                       |
| `role`                                                | [models.WorkspaceRole](../../models/workspacerole.md) | :heavy_check_mark:                                    | N/A                                                   | workspace.member                                      |