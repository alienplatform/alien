# Availability1

## Example Usage

```typescript
import { Availability1 } from "@alienplatform/platform-api/models";

let value: Availability1 = {
  catalogRevision: "<value>",
  models: [],
  source: "azure-foundry",
};
```

## Fields

| Field                                                                                                                                     | Type                                                                                                                                      | Required                                                                                                                                  | Description                                                                                                                               |
| ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `catalogRevision`                                                                                                                         | *string*                                                                                                                                  | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |
| `location`                                                                                                                                | *string*                                                                                                                                  | :heavy_minus_sign:                                                                                                                        | N/A                                                                                                                                       |
| `models`                                                                                                                                  | [models.Model1](../models/model1.md)[]                                                                                                    | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |
| `source`                                                                                                                                  | [models.AvailabilitySource1](../models/availabilitysource1.md)                                                                            | :heavy_check_mark:                                                                                                                        | Provider control plane used to observe model availability without invoking<br/>a model, spending customer quota, or accepting provider terms. |