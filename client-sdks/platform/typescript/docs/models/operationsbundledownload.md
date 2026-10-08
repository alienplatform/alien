# OperationsBundleDownload

## Example Usage

```typescript
import { OperationsBundleDownload } from "@alienplatform/platform-api/models";

let value: OperationsBundleDownload = {
  plugin: "<value>",
  pluginVersion: "<value>",
  url: "https://those-mobility.name",
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `plugin`                                               | *string*                                               | :heavy_check_mark:                                     | N/A                                                    |
| `pluginVersion`                                        | *string*                                               | :heavy_check_mark:                                     | N/A                                                    |
| `url`                                                  | *string*                                               | :heavy_check_mark:                                     | Presigned URL to GET the bundle ZIP from. Short-lived. |