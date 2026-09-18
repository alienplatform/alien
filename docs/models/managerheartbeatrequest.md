# ManagerHeartbeatRequest

## Example Usage

```typescript
import { ManagerHeartbeatRequest } from "@alienplatform/platform-api/models";

let value: ManagerHeartbeatRequest = {
  status: "healthy",
  url: "https://vague-circumference.org",
  managementConfigs: {},
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `status`                                                                             | [models.ManagerHeartbeatRequestStatus](../models/managerheartbeatrequeststatus.md)   | :heavy_check_mark:                                                                   | Current health status                                                                |
| `version`                                                                            | *string*                                                                             | :heavy_minus_sign:                                                                   | Manager version                                                                      |
| `url`                                                                                | *string*                                                                             | :heavy_check_mark:                                                                   | Manager public URL (for accessing DeepStore endpoints)                               |
| `managementConfigs`                                                                  | [models.ManagerManagementConfigs](../models/managermanagementconfigs.md)             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `metrics`                                                                            | [models.ManagerHeartbeatRequestMetrics](../models/managerheartbeatrequestmetrics.md) | :heavy_minus_sign:                                                                   | Optional runtime metrics                                                             |