# TopEndpoint

## Example Usage

```typescript
import { TopEndpoint } from "@aliendotdev/platform-api/models/operations";

let value: TopEndpoint = {
  path: "/Applications",
  method: "<value>",
  statusClass: "<value>",
  requests: 194284,
  latencyP95Ms: 1690.04,
  latencyP99Ms: 8840.72,
  errorRate: 9424.24,
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `path`             | *string*           | :heavy_check_mark: | N/A                |
| `method`           | *string*           | :heavy_check_mark: | N/A                |
| `statusClass`      | *string*           | :heavy_check_mark: | N/A                |
| `requests`         | *number*           | :heavy_check_mark: | N/A                |
| `latencyP95Ms`     | *number*           | :heavy_check_mark: | N/A                |
| `latencyP99Ms`     | *number*           | :heavy_check_mark: | N/A                |
| `errorRate`        | *number*           | :heavy_check_mark: | N/A                |