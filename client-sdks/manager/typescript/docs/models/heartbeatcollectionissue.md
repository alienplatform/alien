# HeartbeatCollectionIssue

## Example Usage

```typescript
import { HeartbeatCollectionIssue } from "@alienplatform/manager-api/models";

let value: HeartbeatCollectionIssue = {
  message: "<value>",
  reason: "not-installed",
  severity: "error",
  source: "<value>",
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `message`                                                                            | *string*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `reason`                                                                             | [models.HeartbeatCollectionIssueReason](../models/heartbeatcollectionissuereason.md) | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `severity`                                                                           | [models.HeartbeatIssueSeverity](../models/heartbeatissueseverity.md)                 | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `source`                                                                             | *string*                                                                             | :heavy_check_mark:                                                                   | N/A                                                                                  |