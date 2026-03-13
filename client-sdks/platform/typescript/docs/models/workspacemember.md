# WorkspaceMember

## Example Usage

```typescript
import { WorkspaceMember } from "@aliendotdev/platform-api/models";

let value: WorkspaceMember = {
  userId: "<id>",
  email: "Carmelo.Ondricka25@yahoo.com",
  name: "<value>",
  image: "https://picsum.photos/seed/Rc5Sgf41/2507/1314",
  role: "workspace.member",
  joinedAt: new Date("2025-11-03T20:38:59.726Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `userId`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `email`                                                                                       | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `name`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `image`                                                                                       | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `role`                                                                                        | [models.WorkspaceRole](../models/workspacerole.md)                                            | :heavy_check_mark:                                                                            | Role for workspace-scoped service accounts                                                    | workspace.member                                                                              |
| `joinedAt`                                                                                    | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |