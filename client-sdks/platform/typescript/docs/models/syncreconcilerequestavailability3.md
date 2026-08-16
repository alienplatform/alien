# SyncReconcileRequestAvailability3

## Example Usage

```typescript
import { SyncReconcileRequestAvailability3 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestAvailability3 = {
  catalogRevision: "<value>",
  models: [
    {
      accessTest: "verified",
      availability: "available",
      blockers: [
        "quota-configuration-required",
      ],
      clientApis: [
        "open-ai-responses",
      ],
      publicModelId: "<id>",
    },
  ],
  source: "gcp-vertex",
};
```

## Fields

| Field                                                                                                                                     | Type                                                                                                                                      | Required                                                                                                                                  | Description                                                                                                                               |
| ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `catalogRevision`                                                                                                                         | *string*                                                                                                                                  | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |
| `location`                                                                                                                                | *string*                                                                                                                                  | :heavy_minus_sign:                                                                                                                        | N/A                                                                                                                                       |
| `models`                                                                                                                                  | [models.SyncReconcileRequestModel3](../models/syncreconcilerequestmodel3.md)[]                                                            | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |
| `source`                                                                                                                                  | [models.SyncReconcileRequestAvailabilitySource3](../models/syncreconcilerequestavailabilitysource3.md)                                    | :heavy_check_mark:                                                                                                                        | Provider control plane used to observe model availability without invoking<br/>a model, spending customer quota, or accepting provider terms. |