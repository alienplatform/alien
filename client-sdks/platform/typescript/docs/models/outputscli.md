# OutputsCli

Outputs from a CLI package build

## Example Usage

```typescript
import { OutputsCli } from "@alienplatform/platform-api/models";

let value: OutputsCli = {
  binaries: {},
  type: "cli",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `binaries`                                                             | Record<string, [models.PackageBinaries](../models/packagebinaries.md)> | :heavy_check_mark:                                                     | Binary information for each target platform                            |
| `type`                                                                 | [models.OutputsTypeCli](../models/outputstypecli.md)                   | :heavy_check_mark:                                                     | N/A                                                                    |