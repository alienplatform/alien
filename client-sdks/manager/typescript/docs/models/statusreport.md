# StatusReport

## Example Usage

```typescript
import { StatusReport } from "@alienplatform/manager-api/models";

let value: StatusReport = {
  state: {},
};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `state`                                                           | [models.State](../models/state.md)                                | :heavy_check_mark:                                                | Deployment state as the environment's Operator last recorded it.  |
| `telemetry`                                                       | [models.ReportedTelemetry](../models/reportedtelemetry.md)[]      | :heavy_minus_sign:                                                | Telemetry the Operator buffered, as the OTLP batches it received. |