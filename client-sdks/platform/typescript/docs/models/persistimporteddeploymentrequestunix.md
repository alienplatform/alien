# PersistImportedDeploymentRequestUnix

Commands for a shell family. Values are never sent to the control plane.

## Example Usage

```typescript
import { PersistImportedDeploymentRequestUnix } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestUnix = {
  inline: "<value>",
  prompt: "<value>",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `inline`                                                                       | *string*                                                                       | :heavy_check_mark:                                                             | Command with a visible value placeholder; shell history can retain it.         |
| `prompt`                                                                       | *string*                                                                       | :heavy_check_mark:                                                             | Hidden-input command, without the value in shell history or process arguments. |