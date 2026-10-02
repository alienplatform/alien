# ReleaseChannelResponse

A release channel.

## Example Usage

```typescript
import { ReleaseChannelResponse } from "@alienplatform/manager-api/models";

let value: ReleaseChannelResponse = {
  deployments: 202491,
  name: "<value>",
  updatedAt: new Date("2024-08-22T02:13:37.786Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `currentReleaseId`                                                                            | *string*                                                                                      | :heavy_minus_sign:                                                                            | Release the channel points at; absent until one is published or promoted to it.               |
| `deployments`                                                                                 | *number*                                                                                      | :heavy_check_mark:                                                                            | Deployments following the channel, pinned or not.                                             |
| `name`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `updatedAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |