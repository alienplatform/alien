# RawHeartbeatSnippet

## Example Usage

```typescript
import { RawHeartbeatSnippet } from "@alienplatform/manager-api/models";

let value: RawHeartbeatSnippet = {
  body: "<value>",
  collectedAt: new Date("2026-06-15T09:33:43.944Z"),
  format: "yaml",
  source: "<value>",
  truncated: false,
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `body`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `collectedAt`                                                                                 | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `format`                                                                                      | [models.RawHeartbeatSnippetFormat](../models/rawheartbeatsnippetformat.md)                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `source`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `truncated`                                                                                   | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |