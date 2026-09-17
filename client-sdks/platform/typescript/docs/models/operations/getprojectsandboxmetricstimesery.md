# GetProjectSandboxMetricsTimeSery

## Example Usage

```typescript
import { GetProjectSandboxMetricsTimeSery } from "@alienplatform/platform-api/models/operations";

let value: GetProjectSandboxMetricsTimeSery = {
  sessions: 611049,
  commands: 65606,
  outcomeUnknown: 236666,
  errors: 645173,
  p95LatencyMs: 3510,
  bucket: new Date("2025-04-25T06:09:13.867Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `sessions`                                                                                    | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `commands`                                                                                    | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `outcomeUnknown`                                                                              | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `errors`                                                                                      | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `p95LatencyMs`                                                                                | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `bucket`                                                                                      | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |