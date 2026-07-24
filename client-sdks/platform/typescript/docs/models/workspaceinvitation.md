# WorkspaceInvitation

## Example Usage

```typescript
import { WorkspaceInvitation } from "@alienplatform/platform-api/models";

let value: WorkspaceInvitation = {
  id: "winv_DsgltMIFV0GmqtxV5NYTtrknrna",
  email: "Lane.Wolff@yahoo.com",
  role: "workspace.member",
  status: "expired",
  deliveryStatus: "sent",
  expiresAt: new Date("2026-06-18T15:21:24.905Z"),
  lastSentAt: new Date("2024-12-31T09:24:38.956Z"),
  createdAt: new Date("2026-03-05T13:06:57.242Z"),
  inviteUrl: "https://impractical-perfection.org/",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the workspace invitation.                                               | winv_DsgltMIFV0GmqtxV5NYTtrknrna                                                              |
| `email`                                                                                       | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `role`                                                                                        | [models.WorkspaceRole](../models/workspacerole.md)                                            | :heavy_check_mark:                                                                            | Role for workspace-scoped service accounts                                                    | workspace.member                                                                              |
| `status`                                                                                      | [models.WorkspaceInvitationStatus](../models/workspaceinvitationstatus.md)                    | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `deliveryStatus`                                                                              | [models.DeliveryStatus](../models/deliverystatus.md)                                          | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `expiresAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `lastSentAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `createdAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `inviteUrl`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
