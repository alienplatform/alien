# HealthCheck

## Example Usage

```typescript
import { HealthCheck } from "@aliendotdev/platform-api/models/operations";

let value: HealthCheck = {
  path: "/usr",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `path`             | *string*           | :heavy_check_mark: | N/A                |
| `port`             | *number*           | :heavy_minus_sign: | N/A                |
| `method`           | *string*           | :heavy_minus_sign: | N/A                |
| `timeoutSeconds`   | *number*           | :heavy_minus_sign: | N/A                |
| `failureThreshold` | *number*           | :heavy_minus_sign: | N/A                |