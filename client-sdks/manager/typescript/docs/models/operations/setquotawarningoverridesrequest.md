# SetQuotaWarningOverridesRequest

## Example Usage

```typescript
import { SetQuotaWarningOverridesRequest } from "@alienplatform/manager-api/models/operations";

let value: SetQuotaWarningOverridesRequest = {
  id: "<id>",
  setQuotaWarningOverridesRequest: {
    quotaWarningOverrides: [
      "<value 1>",
      "<value 2>",
      "<value 3>",
    ],
  },
};
```

## Fields

| Field                                                                                     | Type                                                                                      | Required                                                                                  | Description                                                                               |
| ----------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `id`                                                                                      | *string*                                                                                  | :heavy_check_mark:                                                                        | Deployment ID                                                                             |
| `setQuotaWarningOverridesRequest`                                                         | [models.SetQuotaWarningOverridesRequest](../../models/setquotawarningoverridesrequest.md) | :heavy_check_mark:                                                                        | N/A                                                                                       |