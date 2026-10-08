# TargetOperationsBundleSet

Target operations-bundle set the Operator should converge its loaded plugin registry toward.

## Example Usage

```typescript
import { TargetOperationsBundleSet } from "@alienplatform/platform-api/models";

let value: TargetOperationsBundleSet = {
  hash: "<value>",
  bundles: [],
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `hash`                                                                     | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `bundles`                                                                  | [models.OperationsBundleDownload](../models/operationsbundledownload.md)[] | :heavy_check_mark:                                                         | N/A                                                                        |