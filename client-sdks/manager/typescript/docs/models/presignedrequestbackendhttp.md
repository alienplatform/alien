# PresignedRequestBackendHTTP

HTTP-based request (AWS S3, GCP GCS, Azure Blob)

## Example Usage

```typescript
import { PresignedRequestBackendHTTP } from "@alienplatform/manager-api/models";

let value: PresignedRequestBackendHTTP = {
  headers: {
    "key": "<value>",
    "key1": "<value>",
  },
  method: "<value>",
  type: "http",
  url: "https://cheerful-wafer.info",
};
```

## Fields

| Field                    | Type                     | Required                 | Description              |
| ------------------------ | ------------------------ | ------------------------ | ------------------------ |
| `headers`                | Record<string, *string*> | :heavy_check_mark:       | N/A                      |
| `method`                 | *string*                 | :heavy_check_mark:       | N/A                      |
| `type`                   | *"http"*                 | :heavy_check_mark:       | N/A                      |
| `url`                    | *string*                 | :heavy_check_mark:       | N/A                      |