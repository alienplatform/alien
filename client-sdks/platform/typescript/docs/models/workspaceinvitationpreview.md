# WorkspaceInvitationPreview

## Example Usage

```typescript
import { WorkspaceInvitationPreview } from "@alienplatform/platform-api/models";

let value: WorkspaceInvitationPreview = {
  kind: "link",
  workspace: {
    id: "ws_It13CUaGEhLLAB87simX0",
    name: "<value>",
    logoUrl: "https://deficient-draft.com/",
  },
  inviter: {
    name: "<value>",
    image: "https://grandiose-self-confidence.biz",
  },
  role: "workspace.member",
  expiresAt: new Date("2024-01-12T23:31:33.131Z"),
  state: "revoked",
  emailHint: "<value>",
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    | Example                                                                                        |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `kind`                                                                                         | [models.WorkspaceInvitationPreviewKind](../models/workspaceinvitationpreviewkind.md)           | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `workspace`                                                                                    | [models.WorkspaceInvitationPreviewWorkspace](../models/workspaceinvitationpreviewworkspace.md) | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `inviter`                                                                                      | [models.Inviter](../models/inviter.md)                                                         | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `role`                                                                                         | [models.WorkspaceRole](../models/workspacerole.md)                                             | :heavy_check_mark:                                                                             | Role for workspace-scoped service accounts                                                     | workspace.member                                                                               |
| `expiresAt`                                                                                    | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)  | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `state`                                                                                        | [models.WorkspaceInvitationPreviewState](../models/workspaceinvitationpreviewstate.md)         | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
| `emailHint`                                                                                    | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |                                                                                                |
