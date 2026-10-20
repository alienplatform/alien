# PersistImportedDeploymentRequestCommands

Shell-specific instructions generated beside the secret location.

## Example Usage

```typescript
import { PersistImportedDeploymentRequestCommands } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestCommands = {
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

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `checkCommand`                                                                                         | *string*                                                                                               | :heavy_minus_sign:                                                                                     | Read-only identity check, valid in both shell families.                                                |
| `target`                                                                                               | *string*                                                                                               | :heavy_minus_sign:                                                                                     | Expected account, project, subscription or Kubernetes namespace.                                       |
| `unix`                                                                                                 | [models.PersistImportedDeploymentRequestUnix](../models/persistimporteddeploymentrequestunix.md)       | :heavy_check_mark:                                                                                     | Commands for a shell family. Values are never sent to the control plane.                               |
| `windows`                                                                                              | [models.PersistImportedDeploymentRequestWindows](../models/persistimporteddeploymentrequestwindows.md) | :heavy_check_mark:                                                                                     | Commands for a shell family. Values are never sent to the control plane.                               |