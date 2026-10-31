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

| Field                                                                               | Type                                                                                | Required                                                                            | Description                                                                         |
| ----------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| `status`                                                                            | *string*                                                                            | :heavy_check_mark:                                                                  | N/A                                                                                 |
| `telemetryAccepted`                                                                 | *number*                                                                            | :heavy_check_mark:                                                                  | Telemetry batches passed to the telemetry backend.                                  |
| `telemetryRejected`                                                                 | *number*                                                                            | :heavy_minus_sign:                                                                  | Permanently rejected batches recorded separately for diagnosis, not ingested.       |
| `telemetryThrough`                                                                  | *number*                                                                            | :heavy_minus_sign:                                                                  | Highest batch durably ingested or retained as permanently rejected,<br/>across reports. |
