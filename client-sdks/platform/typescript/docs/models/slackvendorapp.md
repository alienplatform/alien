# SlackVendorApp

## Example Usage

```typescript
import { SlackVendorApp } from "@alienplatform/platform-api/models";

let value: SlackVendorApp = {
  id: "<id>",
  status: "pending",
  name: "<value>",
  description: "for blond quarrel unscramble doubtfully",
  color: "green",
  slackAppId: "<id>",
  botUserId: "<id>",
  connectedByUserId: "<id>",
  connectedAt: "<value>",
  manifestUrl: "https://bowed-supplier.info",
  appSettingsUrl: "https://hateful-strategy.com",
  slackTeamName: "<value>",
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `id`                                                                         | *string*                                                                     | :heavy_check_mark:                                                           | N/A                                                                          |
| `status`                                                                     | [models.SlackVendorAppStatus](../models/slackvendorappstatus.md)             | :heavy_check_mark:                                                           | N/A                                                                          |
| `name`                                                                       | *string*                                                                     | :heavy_check_mark:                                                           | N/A                                                                          |
| `description`                                                                | *string*                                                                     | :heavy_check_mark:                                                           | N/A                                                                          |
| `color`                                                                      | *string*                                                                     | :heavy_check_mark:                                                           | N/A                                                                          |
| `slackAppId`                                                                 | *string*                                                                     | :heavy_check_mark:                                                           | N/A                                                                          |
| `botUserId`                                                                  | *string*                                                                     | :heavy_check_mark:                                                           | N/A                                                                          |
| `connectedByUserId`                                                          | *string*                                                                     | :heavy_check_mark:                                                           | N/A                                                                          |
| `connectedAt`                                                                | *string*                                                                     | :heavy_check_mark:                                                           | N/A                                                                          |
| `manifestUrl`                                                                | *string*                                                                     | :heavy_check_mark:                                                           | Opens Slack's "Create an app" with this app's manifest filled in.            |
| `appSettingsUrl`                                                             | *string*                                                                     | :heavy_check_mark:                                                           | The app's page in Slack's app settings, once connected.                      |
| `slackTeamName`                                                              | *string*                                                                     | :heavy_check_mark:                                                           | The Slack workspace where Alien is installed, where the app must be created. |