# Availability2

## Example Usage

```typescript
import { Availability2 } from "@alienplatform/platform-api/models";

let value: Availability2 = {
  catalogRevision: "<value>",
  models: [
    {
      accessTest: "not-checked",
      availability: "unknown",
      blockers: [],
      clientApis: [],
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
| `models`                                                                                                                                  | [models.Model2](../models/model2.md)[]                                                                                                    | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |
| `source`                                                                                                                                  | [models.AvailabilitySource2](../models/availabilitysource2.md)                                                                            | :heavy_check_mark:                                                                                                                        | Provider control plane used to observe model availability without invoking<br/>a model, spending customer quota, or accepting provider terms. |