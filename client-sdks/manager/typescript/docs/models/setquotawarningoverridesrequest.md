# SetQuotaWarningOverridesRequest

Replace the saved quota warning overrides. An empty list explicitly clears them.

## Example Usage

```typescript
import { SetQuotaWarningOverridesRequest } from "@alienplatform/manager-api/models";

let value: SetQuotaWarningOverridesRequest = {
  quotaWarningOverrides: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
};
```

## Fields

| Field                   | Type                    | Required                | Description             |
| ----------------------- | ----------------------- | ----------------------- | ----------------------- |
| `quotaWarningOverrides` | *string*[]              | :heavy_check_mark:      | N/A                     |