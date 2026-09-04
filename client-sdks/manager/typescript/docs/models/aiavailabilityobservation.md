# AiAvailabilityObservation

## Example Usage

```typescript
import { AiAvailabilityObservation } from "@alienplatform/manager-api/models";

let value: AiAvailabilityObservation = {
  catalogRevision: "<value>",
  models: [
    {
      accessTest: "not-checked",
      availability: "available",
      blockers: [
        "agreement-required",
      ],
      clientApis: [
        "anthropic-messages",
      ],
      publicModelId: "<id>",
    },
  ],
  source: "azure-foundry",
};
```

## Fields

| Field                                                                                                                                     | Type                                                                                                                                      | Required                                                                                                                                  | Description                                                                                                                               |
| ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `catalogRevision`                                                                                                                         | *string*                                                                                                                                  | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |
| `location`                                                                                                                                | *string*                                                                                                                                  | :heavy_minus_sign:                                                                                                                        | N/A                                                                                                                                       |
| `models`                                                                                                                                  | [models.AiModelAvailabilityObservation](../models/aimodelavailabilityobservation.md)[]                                                    | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |
| `source`                                                                                                                                  | [models.AiAvailabilitySource](../models/aiavailabilitysource.md)                                                                          | :heavy_check_mark:                                                                                                                        | Provider control plane used to observe model availability without invoking<br/>a model, spending customer quota, or accepting provider terms. |