# CliOutputs

Outputs from a CLI package build

## Example Usage

```typescript
import { CliOutputs } from "@alienplatform/platform-api/models";

let value: CliOutputs = {
  binaries: {
    "key": {
      sha256: "<value>",
      size: 805179,
      url: "https://essential-advancement.org/",
    },
  },
  buildInfo: {
    alienSha: "<value>",
    horizonSha: "<value>",
    platformSha: "<value>",
    sourceCliBinarySha256: "<value>",
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `binaries`                                                                           | Record<string, [models.DeploymentInfoBinaries](../models/deploymentinfobinaries.md)> | :heavy_check_mark:                                                                   | Binary information for each target platform                                          |
| `buildInfo`                                                                          | [models.DeploymentInfoBuildInfo](../models/deploymentinfobuildinfo.md)               | :heavy_check_mark:                                                                   | Source provenance for a generated CLI package.                                       |