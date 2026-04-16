# DataDeletingAgent

## Example Usage

```typescript
import { DataDeletingAgent } from "@alienplatform/platform-api/models";

let value: DataDeletingAgent = {
  agentId: "<id>",
  releaseId: "<id>",
  type: "DeletingAgent",
};
```

## Fields

| Field                                           | Type                                            | Required                                        | Description                                     |
| ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- |
| `agentId`                                       | *string*                                        | :heavy_check_mark:                              | ID of the agent being deleted                   |
| `releaseId`                                     | *string*                                        | :heavy_check_mark:                              | ID of the release that was running on the agent |
| `type`                                          | *"DeletingAgent"*                               | :heavy_check_mark:                              | N/A                                             |