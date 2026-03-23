# UserSubject

Authenticated user subject with workspace-scoped permissions

## Example Usage

```typescript
import { UserSubject } from "@alienplatform/platform-api/models";

let value: UserSubject = {
  kind: "user",
  id: "<id>",
  email: "King27@yahoo.com",
  workspaceId: "<id>",
  role: "workspace.member",
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            | Example                                                |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `kind`                                                 | *"user"*                                               | :heavy_check_mark:                                     | Subject type identifier                                |                                                        |
| `id`                                                   | *string*                                               | :heavy_check_mark:                                     | Unique user identifier                                 |                                                        |
| `email`                                                | *string*                                               | :heavy_check_mark:                                     | User's email address                                   |                                                        |
| `workspaceId`                                          | *string*                                               | :heavy_check_mark:                                     | ID of the workspace the user is authenticated within   |                                                        |
| `workspaceName`                                        | *string*                                               | :heavy_minus_sign:                                     | Name of the workspace the user is authenticated within |                                                        |
| `role`                                                 | [models.UserRole](../models/userrole.md)               | :heavy_check_mark:                                     | User's role within the workspace                       | workspace.member                                       |