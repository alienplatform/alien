# SlackIntegrationStatus

## Example Usage

```typescript
import { SlackIntegrationStatus } from "@alienplatform/platform-api/models";

let value: SlackIntegrationStatus = {
  connected: true,
  slackTeamId: "<id>",
  slackTeamName: "<value>",
  installedByUserId: "<id>",
  installedAt: "<value>",
  notificationChannelId: "<id>",
};
```

## Fields

| Field                   | Type                    | Required                | Description             |
| ----------------------- | ----------------------- | ----------------------- | ----------------------- |
| `connected`             | *boolean*               | :heavy_check_mark:      | N/A                     |
| `slackTeamId`           | *string*                | :heavy_check_mark:      | N/A                     |
| `slackTeamName`         | *string*                | :heavy_check_mark:      | N/A                     |
| `installedByUserId`     | *string*                | :heavy_check_mark:      | N/A                     |
| `installedAt`           | *string*                | :heavy_check_mark:      | N/A                     |
| `notificationChannelId` | *string*                | :heavy_check_mark:      | N/A                     |
