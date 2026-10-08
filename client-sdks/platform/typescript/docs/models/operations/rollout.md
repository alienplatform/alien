# Rollout

## Example Usage

```typescript
import { Rollout } from "@alienplatform/platform-api/models/operations";

let value: Rollout = {
  releaseId: "<id>",
  version: "<value>",
  upToDate: 786693,
  waiting: 96641,
  unknown: 669432,
  failed: 401240,
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `releaseId`        | *string*           | :heavy_check_mark: | N/A                |
| `version`          | *string*           | :heavy_check_mark: | N/A                |
| `upToDate`         | *number*           | :heavy_check_mark: | N/A                |
| `waiting`          | *number*           | :heavy_check_mark: | N/A                |
| `unknown`          | *number*           | :heavy_check_mark: | N/A                |
| `failed`           | *number*           | :heavy_check_mark: | N/A                |