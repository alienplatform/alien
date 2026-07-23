# EventProgress

Progress information for image push operations

## Example Usage

```typescript
import { EventProgress } from "@alienplatform/platform-api/models";

let value: EventProgress = {
  bytesUploaded: 376832,
  layersUploaded: 873781,
  operation: "<value>",
  totalBytes: 861993,
  totalLayers: 202100,
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
