# CreateAccessRequestApprovedBy

How, when and by whom the request was approved. Null until approved.

## Example Usage

```typescript
import { CreateAccessRequestApprovedBy } from "@alienplatform/platform-api/models/operations";

let value: CreateAccessRequestApprovedBy = {
  method: "<value>",
  actorId: "<id>",
  slack: {
    userId: "<id>",
    userName: "Dusty_Simonis",
    teamId: "<id>",
    teamName: "<value>",
    channelId: "<id>",
    messageTs: "<value>",
    app: "<value>",
  },
  at: "<value>",
};
```

## Fields

| Field                                                                                                                                           | Type                                                                                                                                            | Required                                                                                                                                        | Description                                                                                                                                     |
| ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `method`                                                                                                                                        | *string*                                                                                                                                        | :heavy_check_mark:                                                                                                                              | `kubectl` (in the customer's cluster), `slack`, or the surface of a dashboard or API approval (`dashboard`, `cli`).                             |
| `actorId`                                                                                                                                       | *string*                                                                                                                                        | :heavy_check_mark:                                                                                                                              | The Alien user who approved in the dashboard or API. Null for kubectl (the approver is only in the customer's cluster audit log) and for Slack. |
| `slack`                                                                                                                                         | [models.AccessRequestSlackDecider](../../models/accessrequestslackdecider.md)                                                                   | :heavy_check_mark:                                                                                                                              | N/A                                                                                                                                             |
| `at`                                                                                                                                            | *string*                                                                                                                                        | :heavy_check_mark:                                                                                                                              | N/A                                                                                                                                             |