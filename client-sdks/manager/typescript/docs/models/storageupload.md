# StorageUpload

Storage upload information

## Example Usage

```typescript
import { StorageUpload } from "@alienplatform/manager-api/models";

let value: StorageUpload = {
  expiresAt: new Date("2026-07-10T23:59:06.699Z"),
  putRequest: {
    backend: {
      headers: {
        "key": "<value>",
        "key1": "<value>",
      },
      method: "<value>",
      type: "http",
      url: "https://comfortable-receptor.info",
    },
    expiration: new Date("2024-07-04T18:30:14.975Z"),
    operation: "get",
    path: "/proc",
  },
};
```

## Fields

| Field                                                                                                                                | Type                                                                                                                                 | Required                                                                                                                             | Description                                                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `expiresAt`                                                                                                                          | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                                        | :heavy_check_mark:                                                                                                                   | Expiration time for upload URL                                                                                                       |
| `putRequest`                                                                                                                         | [models.PresignedRequest](../models/presignedrequest.md)                                                                             | :heavy_check_mark:                                                                                                                   | A presigned request that can be serialized, stored, and executed later.<br/>Hides implementation details for different storage backends. |