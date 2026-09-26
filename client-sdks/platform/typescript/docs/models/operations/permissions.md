# Permissions

Installed cloud and Kubernetes permissions. Setup applies them, so enabling or disabling operations changes them only after the installation's setup is re-applied.

## Example Usage

```typescript
import { Permissions } from "@alienplatform/platform-api/models/operations";

let value: Permissions = {
  status: "unknown",
  recordedAt: new Date("2025-06-28T18:34:49.070Z"),
  reason: "<value>",
};
```

## Fields

| Field                                                                                                                                          | Type                                                                                                                                           | Required                                                                                                                                       | Description                                                                                                                                    |
| ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `status`                                                                                                                                       | [operations.PermissionsStatus](../../models/operations/permissionsstatus.md)                                                                   | :heavy_check_mark:                                                                                                                             | Whether the Kubernetes RBAC and cloud IAM recorded for this installation match the permissions compiled from the currently enabled operations. |
| `recordedAt`                                                                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                                                  | :heavy_check_mark:                                                                                                                             | When setup was issued or last re-applied; null when nothing was recorded.                                                                      |
| `reason`                                                                                                                                       | *string*                                                                                                                                       | :heavy_check_mark:                                                                                                                             | N/A                                                                                                                                            |