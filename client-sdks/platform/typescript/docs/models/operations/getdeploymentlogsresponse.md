# GetDeploymentLogsResponse

Up to 100 recent log entries.

## Example Usage

```typescript
import { GetDeploymentLogsResponse } from "@alienplatform/platform-api/models/operations";

let value: GetDeploymentLogsResponse = {
  logs: [
    {
      timestamp: "<value>",
      level: "<value>",
      message: "<value>",
    },
  ],
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `logs`                                                                               | [operations.GetDeploymentLogsLog](../../models/operations/getdeploymentlogslog.md)[] | :heavy_check_mark:                                                                   | N/A                                                                                  |