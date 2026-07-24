# WorkspaceInviteLink

## Example Usage

```typescript
import { WorkspaceInviteLink } from "@alienplatform/platform-api/models";

let value: WorkspaceInviteLink = {
  id: "wil_RgcthDSZ37rmFLekuItpFS7btjXoYwou1gE4",
  role: "workspace.member",
  expiresAt: new Date("2025-08-10T09:44:19.208Z"),
  createdAt: new Date("2024-12-17T22:24:34.536Z"),
  useCount: 786384,
  lastUsedAt: new Date("2024-02-08T13:27:28.425Z"),
  inviteUrl: "https://immediate-drug.net/",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the workspace invite link.                                              | wil_RgcthDSZ37rmFLekuItpFS7btjXoYwou1gE4                                                      |
| `role`                                                                                        | [models.WorkspaceRole](../models/workspacerole.md)                                            | :heavy_check_mark:                                                                            | Role for workspace-scoped service accounts                                                    | workspace.member                                                                              |
| `expiresAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `createdAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `useCount`                                                                                    | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `lastUsedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `inviteUrl`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
