# AgentSessionListItem

## Example Usage

```typescript
import { AgentSessionListItem } from "@alienplatform/platform-api/models";

let value: AgentSessionListItem = {
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
  createdAt: "1709761239116",
  updatedAt: "1735623031963",
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
