# AiAvailabilityObservation

## Example Usage

```typescript
import { AiAvailabilityObservation } from "@alienplatform/platform-api/models";

let value: AiAvailabilityObservation = {
  catalogRevision: "<value>",
  models: [
    {
      accessTest: "not-checked",
      availability: "available",
      blockers: [],
      clientApis: [],
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
| `models`                                                                                                                                  | [models.AiAvailabilityObservationModel](../models/aiavailabilityobservationmodel.md)[]                                                    | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |
| `source`                                                                                                                                  | [models.AiAvailabilityObservationSource](../models/aiavailabilityobservationsource.md)                                                    | :heavy_check_mark:                                                                                                                        | Provider control plane used to observe model availability without invoking<br/>a model, spending customer quota, or accepting provider terms. |