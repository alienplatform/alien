# EventListItemResponseDataUpdatingAgent

## Example Usage

```typescript
import { EventListItemResponseDataUpdatingAgent } from "@alienplatform/platform-api/models";

let value: EventListItemResponseDataUpdatingAgent = {
  agentId: "<id>",
  releaseId: "<id>",
  type: "UpdatingAgent",
};
```

## Fields

| Field                                             | Type                                              | Required                                          | Description                                       |
| ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- |
| `agentId`                                         | *string*                                          | :heavy_check_mark:                                | ID of the agent being updated                     |
| `releaseId`                                       | *string*                                          | :heavy_check_mark:                                | ID of the new release being deployed to the agent |
| `type`                                            | *"UpdatingAgent"*                                 | :heavy_check_mark:                                | N/A                                               |
