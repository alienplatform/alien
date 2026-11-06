# DeploymentTrafficRecord

## Example Usage

```typescript
import { DeploymentTrafficRecord } from "@alienplatform/platform-api/models";

let value: DeploymentTrafficRecord = {
  resourceId: "<id>",
  endpoint: "<value>",
  name: "<value>",
  type: "CNAME",
  target: "<value>",
  status: "unknown",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `resourceId`                                                                       | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `endpoint`                                                                         | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `name`                                                                             | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `type`                                                                             | [models.DeploymentTrafficRecordType](../models/deploymenttrafficrecordtype.md)     | :heavy_check_mark:                                                                 | N/A                                                                                |
| `target`                                                                           | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `status`                                                                           | [models.DeploymentTrafficRecordStatus](../models/deploymenttrafficrecordstatus.md) | :heavy_check_mark:                                                                 | N/A                                                                                |