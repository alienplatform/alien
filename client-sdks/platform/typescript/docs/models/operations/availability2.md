# Availability2

## Example Usage

```typescript
import { Availability2 } from "@alienplatform/platform-api/models/operations";

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
| `models`                                                                                                                                  | [operations.GetResourceDeploymentDetailModel2](../../models/operations/getresourcedeploymentdetailmodel2.md)[]                            | :heavy_check_mark:                                                                                                                        | N/A                                                                                                                                       |
| `source`                                                                                                                                  | [operations.SourceEnum2](../../models/operations/sourceenum2.md)                                                                          | :heavy_check_mark:                                                                                                                        | Provider control plane used to observe model availability without invoking<br/>a model, spending customer quota, or accepting provider terms. |