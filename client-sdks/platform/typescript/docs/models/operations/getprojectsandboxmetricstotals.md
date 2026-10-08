# GetProjectSandboxMetricsTotals

## Example Usage

```typescript
import { GetProjectSandboxMetricsTotals } from "@alienplatform/platform-api/models/operations";

let value: GetProjectSandboxMetricsTotals = {
  sessions: 424979,
  commands: 254437,
  outcomeUnknown: 721981,
  errors: 432230,
  p95LatencyMs: 3915.5,
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `sessions`         | *number*           | :heavy_check_mark: | N/A                |
| `commands`         | *number*           | :heavy_check_mark: | N/A                |
| `outcomeUnknown`   | *number*           | :heavy_check_mark: | N/A                |
| `errors`           | *number*           | :heavy_check_mark: | N/A                |
| `p95LatencyMs`     | *number*           | :heavy_check_mark: | N/A                |