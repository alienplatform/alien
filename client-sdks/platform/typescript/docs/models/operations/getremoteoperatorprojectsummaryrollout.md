# GetRemoteOperatorProjectSummaryRollout

## Example Usage

```typescript
import { GetRemoteOperatorProjectSummaryRollout } from "@alienplatform/platform-api/models/operations";

let value: GetRemoteOperatorProjectSummaryRollout = {
  releaseId: "<id>",
  version: "<value>",
  upToDate: 917540,
  waiting: 349864,
  unknown: 669054,
  failed: 919347,
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