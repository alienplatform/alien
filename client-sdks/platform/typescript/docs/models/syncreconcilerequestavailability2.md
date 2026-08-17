# SyncReconcileRequestAvailability2

## Example Usage

```typescript
import { SyncReconcileRequestAvailability2 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestAvailability2 = {
  catalogRevision: "<value>",
  models: [
    {
      accessTest: "failed",
      availability: "blocked",
      blockers: [],
      clientApis: [
        "open-ai-chat-completions",
      ],
      publicModelId: "<id>",
    },
  ],
  source: "aws-bedrock",
};
```

## Fields

| Field                                                                                                                                     | Type                                                                                                                                      | Required                                                                                                                                  | Description                                                                                                                               |
| ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `catalogRevision`                                                                                                                         | *string*                                                                                                                                  | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |
| `location`                                                                                                                                | *string*                                                                                                                                  | :heavy_minus_sign:                                                                                                                        | N/A                                                                                                                                       |
| `models`                                                                                                                                  | [models.SyncReconcileRequestModel2](../models/syncreconcilerequestmodel2.md)[]                                                            | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |
| `source`                                                                                                                                  | [models.AvailabilitySource2](../models/availabilitysource2.md)                                                                            | :heavy_check_mark:                                                                                                                        | Provider control plane used to observe model availability without invoking<br/>a model, spending customer quota, or accepting provider terms. |