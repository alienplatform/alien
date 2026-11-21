# ListAccessRequestsDeniedBy

## Example Usage

```typescript
import { ListAccessRequestsDeniedBy } from "@alienplatform/platform-api/models/operations";

let value: ListAccessRequestsDeniedBy = {
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

| Field                                                                         | Type                                                                          | Required                                                                      | Description                                                                   |
| ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| `actorId`                                                                     | *string*                                                                      | :heavy_check_mark:                                                            | The Alien user who denied in the dashboard or API. Null for Slack.            |
| `slack`                                                                       | [models.AccessRequestSlackDecider](../../models/accessrequestslackdecider.md) | :heavy_check_mark:                                                            | N/A                                                                           |
| `at`                                                                          | *string*                                                                      | :heavy_check_mark:                                                            | N/A                                                                           |