# CreateAccessRequestDeniedBy

## Example Usage

```typescript
import { CreateAccessRequestDeniedBy } from "@alienplatform/platform-api/models/operations";

let value: CreateAccessRequestDeniedBy = {
  actorId: "<id>",
  slack: null,
  at: "<value>",
};
```

## Fields

| Field                                                                         | Type                                                                          | Required                                                                      | Description                                                                   |
| ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| `actorId`                                                                     | *string*                                                                      | :heavy_check_mark:                                                            | The Alien user who denied in the dashboard or API. Null for Slack.            |
| `slack`                                                                       | [models.AccessRequestSlackDecider](../../models/accessrequestslackdecider.md) | :heavy_check_mark:                                                            | N/A                                                                           |
| `at`                                                                          | *string*                                                                      | :heavy_check_mark:                                                            | N/A                                                                           |