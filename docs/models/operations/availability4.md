# Availability4

## Example Usage

```typescript
import { Availability4 } from "@alienplatform/platform-api/models/operations";

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
| `models`                                                                                                                                  | [operations.GetResourceDeploymentDetailModel4](../../models/operations/getresourcedeploymentdetailmodel4.md)[]                            | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |
| `source`                                                                                                                                  | [operations.SourceEnum4](../../models/operations/sourceenum4.md)                                                                          | :heavy_check_mark:                                                                                                                        | Provider control plane used to observe model availability without invoking<br/>a model, spending customer quota, or accepting provider terms. |