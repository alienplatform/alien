# OutputsCli

Outputs from a CLI package build

## Example Usage

```typescript
import { OutputsCli } from "@alienplatform/platform-api/models";

let value: OutputsCli = {
  binaries: {},
  buildInfo: {
    alienSha: "<value>",
    horizonSha: "<value>",
    platformSha: "<value>",
    sourceAgentBinarySha256: "<value>",
    sourceCliBinarySha256: "<value>",
  },
  type: "cli",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `binaries`                                                             | Record<string, [models.PackageBinaries](../models/packagebinaries.md)> | :heavy_check_mark:                                                     | Binary information for each target platform                            |
| `buildInfo`                                                            | [models.PackageBuildInfo](../models/packagebuildinfo.md)               | :heavy_check_mark:                                                     | Source provenance for a generated CLI package.                         |
| `type`                                                                 | [models.OutputsTypeCli](../models/outputstypecli.md)                   | :heavy_check_mark:                                                     | N/A                                                                    |