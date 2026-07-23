# AgentSessionApprovalGrantedEventPayload

## Example Usage

```typescript
import { AgentSessionApprovalGrantedEventPayload } from "@alienplatform/platform-api/models";

let value: AgentSessionApprovalGrantedEventPayload = {
  approvalId: "<id>",
  approvedByUserId: "<id>",
  approvedByName: "<value>",
  source: "dashboard",
};
```

## Fields

| Field                                        | Type                                         | Required                                     | Description                                  |
| -------------------------------------------- | -------------------------------------------- | -------------------------------------------- | -------------------------------------------- |
| `approvalId`                                 | *string*                                     | :heavy_check_mark:                           | N/A                                          |
| `approvedByUserId`                           | *string*                                     | :heavy_check_mark:                           | N/A                                          |
| `approvedByName`                             | *string*                                     | :heavy_check_mark:                           | N/A                                          |
| `source`                                     | [models.SourceEnum](../models/sourceenum.md) | :heavy_check_mark:                           | N/A                                          |
