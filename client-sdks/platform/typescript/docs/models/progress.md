# Progress

Progress information for image push operations

## Example Usage

```typescript
import { Progress } from "@aliendotdev/platform-api/models";

let value: Progress = {
  bytesUploaded: 641278,
  layersUploaded: 340252,
  operation: "<value>",
  totalBytes: 238665,
  totalLayers: 685450,
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