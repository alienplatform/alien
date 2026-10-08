# EventListItemResponseDataReleaseChannelUpdated

## Example Usage

```typescript
import { EventListItemResponseDataReleaseChannelUpdated } from "@alienplatform/platform-api/models";

let value: EventListItemResponseDataReleaseChannelUpdated = {
  channel: "<value>",
  releaseId: "<id>",
  type: "ReleaseChannelUpdated",
};
```

## Fields

| Field                                                 | Type                                                  | Required                                              | Description                                           |
| ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- |
| `actor`                                               | *models.EventListItemResponseActorUnion1*             | :heavy_minus_sign:                                    | N/A                                                   |
| `channel`                                             | *string*                                              | :heavy_check_mark:                                    | Name of the channel that moved                        |
| `previousReleaseId`                                   | *string*                                              | :heavy_minus_sign:                                    | ID of the release that was previously current, if any |
| `releaseId`                                           | *string*                                              | :heavy_check_mark:                                    | ID of the release that is now current                 |
| `type`                                                | *"ReleaseChannelUpdated"*                             | :heavy_check_mark:                                    | N/A                                                   |