# CliOutputs

Outputs from a CLI package build

## Example Usage

```typescript
import { CliOutputs } from "@aliendotdev/platform-api/models";

let value: CliOutputs = {
  binaries: {
    "key": {
      sha256: "<value>",
      size: 805179,
      url: "https://essential-advancement.org/",
    },
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `binaries`                                                                           | Record<string, [models.DeploymentInfoBinaries](../models/deploymentinfobinaries.md)> | :heavy_check_mark:                                                                   | Binary information for each target platform                                          |