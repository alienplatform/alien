# TargetOperationsBundleSet

Target operations-bundle set for the Operator to converge its loaded
plugin registry toward, independent of any release/config target — a
plugin can be enabled with no release change, so this is not nested under
`TargetDeployment`.

## Example Usage

```typescript
import { TargetOperationsBundleSet } from "@alienplatform/manager-api/models";

let value: TargetOperationsBundleSet = {
  hash: "<value>",
};
```

## Fields

| Field                                                                                                                                                                                               | Type                                                                                                                                                                                                | Required                                                                                                                                                                                            | Description                                                                                                                                                                                         |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `bundles`                                                                                                                                                                                           | [models.OperationsBundleDownload](../models/operationsbundledownload.md)[]                                                                                                                          | :heavy_minus_sign:                                                                                                                                                                                  | Presigned downloads for every bundle in the target set. The Operator<br/>fetches only the ones it doesn't already have loaded at the right<br/>version; already-loaded bundles are harmless to re-download. |
| `hash`                                                                                                                                                                                              | *string*                                                                                                                                                                                            | :heavy_check_mark:                                                                                                                                                                                  | Hash identifying this exact enabled-plugin set. Compare against<br/>`OperationsReport.loadedBundleHash` to detect drift.                                                                            |