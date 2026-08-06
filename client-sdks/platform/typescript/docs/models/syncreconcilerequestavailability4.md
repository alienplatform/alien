# SyncReconcileRequestAvailability4

## Example Usage

```typescript
import { SyncReconcileRequestAvailability4 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestAvailability4 = {
  catalogRevision: "<value>",
  models: [],
  source: "aws-bedrock",
};
```

## Fields

| Field                                                                                                                                     | Type                                                                                                                                      | Required                                                                                                                                  | Description                                                                                                                               |
| ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `catalogRevision`                                                                                                                         | *string*                                                                                                                                  | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |
| `location`                                                                                                                                | *string*                                                                                                                                  | :heavy_minus_sign:                                                                                                                        | N/A                                                                                                                                       |
| `models`                                                                                                                                  | [models.SyncReconcileRequestModel4](../models/syncreconcilerequestmodel4.md)[]                                                            | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |
| `source`                                                                                                                                  | [models.SyncReconcileRequestAvailabilitySource4](../models/syncreconcilerequestavailabilitysource4.md)                                    | :heavy_check_mark:                                                                                                                        | Provider control plane used to observe model availability without invoking<br/>a model, spending customer quota, or accepting provider terms. |