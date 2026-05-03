# ManagerHeartbeatRequest

## Example Usage

```typescript
import { ManagerHeartbeatRequest } from "@alienplatform/platform-api/models";

let value: ManagerHeartbeatRequest = {
  status: "healthy",
  url: "https://vague-circumference.org",
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `status`                                                                             | [models.ManagerHeartbeatRequestStatus](../models/managerheartbeatrequeststatus.md)   | :heavy_check_mark:                                                                   | Current health status                                                                |
| `version`                                                                            | *string*                                                                             | :heavy_minus_sign:                                                                   | Manager version                                                                      |
| `url`                                                                                | *string*                                                                             | :heavy_check_mark:                                                                   | Manager public URL (for accessing DeepStore endpoints)                               |
| `managementConfig`                                                                   | *models.ManagerHeartbeatRequestManagementConfigUnion*                                | :heavy_minus_sign:                                                                   | Management configuration for cross-account access (from ServiceAccount binding)      |
| `metrics`                                                                            | [models.ManagerHeartbeatRequestMetrics](../models/managerheartbeatrequestmetrics.md) | :heavy_minus_sign:                                                                   | Optional runtime metrics                                                             |