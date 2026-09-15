# GetProjectSandboxMetricsAvailable

## Example Usage

```typescript
import { GetProjectSandboxMetricsAvailable } from "@alienplatform/platform-api/models/operations";

let value: GetProjectSandboxMetricsAvailable = {
  status: "available",
  range: "24h",
  totals: {
    sessions: 247824,
    commands: 998578,
    outcomeUnknown: 905229,
    errors: 59188,
    p95LatencyMs: 813.24,
  },
  timeSeries: [],
  customers: [],
  customersTruncated: false,
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `status`                                                                                                             | *"available"*                                                                                                        | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `range`                                                                                                              | [operations.GetProjectSandboxMetricsRangeResponse](../../models/operations/getprojectsandboxmetricsrangeresponse.md) | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `totals`                                                                                                             | [operations.GetProjectSandboxMetricsTotals](../../models/operations/getprojectsandboxmetricstotals.md)               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `timeSeries`                                                                                                         | [operations.GetProjectSandboxMetricsTimeSery](../../models/operations/getprojectsandboxmetricstimesery.md)[]         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `customers`                                                                                                          | [operations.GetProjectSandboxMetricsCustomer](../../models/operations/getprojectsandboxmetricscustomer.md)[]         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `customersTruncated`                                                                                                 | *boolean*                                                                                                            | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |