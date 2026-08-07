# Availability4

## Example Usage

```typescript
import { Availability4 } from "@alienplatform/platform-api/models";

let value: Availability4 = {
  catalogRevision: "<value>",
  models: [
    {
      accessTest: "verified",
      availability: "unknown",
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
| `models`                                                                                                                                  | [models.Model4](../models/model4.md)[]                                                                                                    | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |
| `source`                                                                                                                                  | [models.AvailabilitySource4](../models/availabilitysource4.md)                                                                            | :heavy_check_mark:                                                                                                                        | Provider control plane used to observe model availability without invoking<br/>a model, spending customer quota, or accepting provider terms. |