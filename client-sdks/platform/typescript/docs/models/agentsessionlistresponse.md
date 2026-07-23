# AgentSessionListResponse

## Example Usage

```typescript
import { AgentSessionListResponse } from "@alienplatform/platform-api/models";

let value: AgentSessionListResponse = {
  sessions: [
    {
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
      createdAt: "1711341083645",
      updatedAt: "1735641395451",
    },
  ],
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `sessions`                                                         | [models.AgentSessionListItem](../models/agentsessionlistitem.md)[] | :heavy_check_mark:                                                 | N/A                                                                |
