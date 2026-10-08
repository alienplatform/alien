# PackageRuntimePersistence

Operator identity storage in the generated chart. This does not install a storage driver.

## Example Usage

```typescript
import { PackageRuntimePersistence } from "@alienplatform/platform-api/models";

let value: PackageRuntimePersistence = {
  enabled: true,
  existingClaim: "<value>",
  size: "<value>",
  storageClassName: "<value>",
};
```

## Fields

| Field                                                     | Type                                                      | Required                                                  | Description                                               |
| --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- |
| `enabled`                                                 | *boolean*                                                 | :heavy_check_mark:                                        | Persist operator identity across restarts.                |
| `existingClaim`                                           | *string*                                                  | :heavy_check_mark:                                        | Existing namespace claim instead of creating a new claim. |
| `size`                                                    | *string*                                                  | :heavy_check_mark:                                        | Requested claim capacity, for example 1Gi.                |
| `storageClassName`                                        | *string*                                                  | :heavy_check_mark:                                        | Cluster storage class; empty uses the cluster default.    |
