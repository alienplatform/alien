# StatusReportResponse

## Example Usage

```typescript
import { StatusReportResponse } from "@alienplatform/manager-api/models";

let value: StatusReportResponse = {
  status: "<value>",
  telemetryAccepted: 198699,
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `status`                                                                     | *string*                                                                     | :heavy_check_mark:                                                           | N/A                                                                          |
| `telemetryAccepted`                                                          | *number*                                                                     | :heavy_check_mark:                                                           | Telemetry batches passed to the telemetry backend.                           |
| `telemetryThrough`                                                           | *number*                                                                     | :heavy_minus_sign:                                                           | Highest batch the manager has passed on for this deployment, across<br/>reports. |