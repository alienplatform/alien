# DeploymentDetailResponseCommands

Shell-specific instructions generated beside the secret location.

## Example Usage

```typescript
import { DeploymentDetailResponseCommands } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseCommands = {
  unix: {
    inline: "<value>",
    prompt: "<value>",
  },
  windows: {
    inline: "<value>",
    prompt: "<value>",
  },
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `checkCommand`                                                                         | *string*                                                                               | :heavy_minus_sign:                                                                     | Read-only identity check, valid in both shell families.                                |
| `target`                                                                               | *string*                                                                               | :heavy_minus_sign:                                                                     | Expected account, project, subscription or Kubernetes namespace.                       |
| `unix`                                                                                 | [models.DeploymentDetailResponseUnix](../models/deploymentdetailresponseunix.md)       | :heavy_check_mark:                                                                     | Commands for a shell family. Values are never sent to the control plane.               |
| `windows`                                                                              | [models.DeploymentDetailResponseWindows](../models/deploymentdetailresponsewindows.md) | :heavy_check_mark:                                                                     | Commands for a shell family. Values are never sent to the control plane.               |