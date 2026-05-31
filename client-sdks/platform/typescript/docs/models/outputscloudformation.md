# OutputsCloudformation

Outputs from a CloudFormation package build.

## Example Usage

```typescript
import { OutputsCloudformation } from "@alienplatform/platform-api/models";

let value: OutputsCloudformation = {
  targets: {},
  type: "cloudformation",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `targets`                                                                  | Record<string, [models.PackageTargets](../models/packagetargets.md)>       | :heavy_check_mark:                                                         | Template artifacts by CloudFormation target.                               |
| `type`                                                                     | [models.OutputsTypeCloudformation](../models/outputstypecloudformation.md) | :heavy_check_mark:                                                         | N/A                                                                        |