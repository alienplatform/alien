# StatusReport

## Example Usage

```typescript
import { StatusReport } from "@alienplatform/manager-api/models";

let value: StatusReport = {
  state: {
    "key": "New Hampshire",
  },
};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `state`                                                           | Record<string, *any*>                                             | :heavy_check_mark:                                                | Deployment state as the environment's Operator last recorded it.  |
| `telemetry`                                                       | [models.ReportedTelemetry](../models/reportedtelemetry.md)[]      | :heavy_minus_sign:                                                | Telemetry the Operator buffered, as the OTLP batches it received. |