# AcceptWorkspaceInvitationResponse

## Example Usage

```typescript
import { AcceptWorkspaceInvitationResponse } from "@alienplatform/platform-api/models";

let value: AcceptWorkspaceInvitationResponse = {
  outcome: "joined",
  workspaceId: "ws_It13CUaGEhLLAB87simX0",
  workspaceName: "<value>",
  role: "workspace.member",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        | Example                                            |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `outcome`                                          | [models.Outcome](../models/outcome.md)             | :heavy_check_mark:                                 | N/A                                                |                                                    |
| `workspaceId`                                      | *string*                                           | :heavy_check_mark:                                 | Unique identifier for the workspace.               | ws_It13CUaGEhLLAB87simX0                           |
| `workspaceName`                                    | *string*                                           | :heavy_check_mark:                                 | N/A                                                |                                                    |
| `role`                                             | [models.WorkspaceRole](../models/workspacerole.md) | :heavy_check_mark:                                 | Role for workspace-scoped service accounts         | workspace.member                                   |
