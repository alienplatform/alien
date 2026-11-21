# SyncReconcileResponseValueSlot

## Example Usage

```typescript
import { SyncReconcileResponseValueSlot } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseValueSlot = {
  slot: "<value>",
  kind: "secret",
  digest: "<value>",
  envName: "<value>",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `slot`                                                                     | *string*                                                                   | :heavy_check_mark:                                                         | Slot name of a dynamic container's secret environment variable.            |
| `kind`                                                                     | [models.SyncReconcileResponseKind](../models/syncreconcileresponsekind.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `digest`                                                                   | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `envName`                                                                  | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |