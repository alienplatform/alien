# AgentSessionApprovalGrantedEvent

## Example Usage

```typescript
import { AgentSessionApprovalGrantedEvent } from "@alienplatform/platform-api/models";

let value: AgentSessionApprovalGrantedEvent = {
  seq: 2155.04,
  createdAt: "1722031798553",
  type: "approval_granted",
  payload: {
    approvalId: "<id>",
    approvedByUserId: "<id>",
    approvedByName: "<value>",
    source: "dashboard",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `seq`                                                                                                  | *number*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `createdAt`                                                                                            | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `type`                                                                                                 | *"approval_granted"*                                                                                   | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `payload`                                                                                              | [models.AgentSessionApprovalGrantedEventPayload](../models/agentsessionapprovalgrantedeventpayload.md) | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
