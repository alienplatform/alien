# EventListItemResponseProgress

Progress information for image push operations

## Example Usage

```typescript
import { EventListItemResponseProgress } from "@alienplatform/platform-api/models";

let value: EventListItemResponseProgress = {
  bytesUploaded: 810913,
  layersUploaded: 441338,
  operation: "<value>",
  totalBytes: 43935,
  totalLayers: 949681,
};
```

## Fields

| Field                             | Type                              | Required                          | Description                       |
| --------------------------------- | --------------------------------- | --------------------------------- | --------------------------------- |
| `bytesUploaded`                   | *number*                          | :heavy_check_mark:                | Bytes uploaded so far             |
| `layersUploaded`                  | *number*                          | :heavy_check_mark:                | Number of layers uploaded so far  |
| `operation`                       | *string*                          | :heavy_check_mark:                | Current operation being performed |
| `totalBytes`                      | *number*                          | :heavy_check_mark:                | Total bytes to upload             |
| `totalLayers`                     | *number*                          | :heavy_check_mark:                | Total number of layers to upload  |
