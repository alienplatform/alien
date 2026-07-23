# AgentSessionDetail

## Example Usage

```typescript
import { AgentSessionDetail } from "@alienplatform/platform-api/models";

let value: AgentSessionDetail = {
  id: "<id>",
  triggerType: "<value>",
  subjectId: "<id>",
  subject: {
    deploymentName: "<value>",
    deploymentGroupId: null,
    deploymentGroupName: "<value>",
    releaseId: "<id>",
    releaseCommitMessage: "<value>",
    releaseCommitRef: "<value>",
    projectId: "<id>",
    projectName: "<value>",
  },
  status: "<value>",
  createdAt: "1708547203760",
  updatedAt: "1735665644852",
  resultText: "<value>",
  toolNames: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  error: null,
  pendingApproval: {
    approvalId: "<id>",
    toolCallId: "<id>",
    toolName: "<value>",
  },
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `id`                                                           | *string*                                                       | :heavy_check_mark:                                             | N/A                                                            |
| `triggerType`                                                  | *string*                                                       | :heavy_check_mark:                                             | N/A                                                            |
| `subjectId`                                                    | *string*                                                       | :heavy_check_mark:                                             | N/A                                                            |
| `subject`                                                      | [models.AgentSessionSubject](../models/agentsessionsubject.md) | :heavy_check_mark:                                             | N/A                                                            |
| `status`                                                       | *string*                                                       | :heavy_check_mark:                                             | N/A                                                            |
| `createdAt`                                                    | *string*                                                       | :heavy_check_mark:                                             | N/A                                                            |
| `updatedAt`                                                    | *string*                                                       | :heavy_check_mark:                                             | N/A                                                            |
| `resultText`                                                   | *string*                                                       | :heavy_check_mark:                                             | N/A                                                            |
| `toolNames`                                                    | *string*[]                                                     | :heavy_check_mark:                                             | N/A                                                            |
| `error`                                                        | *string*                                                       | :heavy_check_mark:                                             | N/A                                                            |
| `pendingApproval`                                              | [models.PendingApproval](../models/pendingapproval.md)         | :heavy_check_mark:                                             | N/A                                                            |
