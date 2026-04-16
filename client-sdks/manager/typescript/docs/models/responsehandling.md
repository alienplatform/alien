# ResponseHandling

Response handling configuration for deployments

## Example Usage

```typescript
import { ResponseHandling } from "@alienplatform/manager-api/models";

let value: ResponseHandling = {
  maxInlineBytes: 158942,
  storageUploadRequest: {
    backend: {
      filePath: "/private/var/anenst.mar",
      operation: "delete",
      type: "local",
    },
    expiration: new Date("2026-06-22T00:08:29.133Z"),
    operation: "delete",
    path: "/usr",
  },
  submitResponseUrl: "https://silent-steeple.biz/",
};
```

## Fields

| Field                                                                                                                                | Type                                                                                                                                 | Required                                                                                                                             | Description                                                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `maxInlineBytes`                                                                                                                     | *number*                                                                                                                             | :heavy_check_mark:                                                                                                                   | Maximum response body size that can be submitted inline                                                                              |
| `storageUploadRequest`                                                                                                               | [models.PresignedRequest](../models/presignedrequest.md)                                                                             | :heavy_check_mark:                                                                                                                   | A presigned request that can be serialized, stored, and executed later.<br/>Hides implementation details for different storage backends. |
| `submitResponseUrl`                                                                                                                  | *string*                                                                                                                             | :heavy_check_mark:                                                                                                                   | URL where deployments submit responses                                                                                               |