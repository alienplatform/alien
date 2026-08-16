# SyncReconcileRequestAvailability1

## Example Usage

```typescript
import { SyncReconcileRequestAvailability1 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestAvailability1 = {
  catalogRevision: "<value>",
  models: [],
  source: "gcp-vertex",
};
```

## Fields

| Field                                                                                                                                     | Type                                                                                                                                      | Required                                                                                                                                  | Description                                                                                                                               |
| ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `catalogRevision`                                                                                                                         | *string*                                                                                                                                  | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |
| `location`                                                                                                                                | *string*                                                                                                                                  | :heavy_minus_sign:                                                                                                                        | N/A                                                                                                                                       |
| `models`                                                                                                                                  | [models.SyncReconcileRequestModel1](../models/syncreconcilerequestmodel1.md)[]                                                            | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |
| `source`                                                                                                                                  | [models.SyncReconcileRequestAvailabilitySource1](../models/syncreconcilerequestavailabilitysource1.md)                                    | :heavy_check_mark:                                                                                                                        | Provider control plane used to observe model availability without invoking<br/>a model, spending customer quota, or accepting provider terms. |