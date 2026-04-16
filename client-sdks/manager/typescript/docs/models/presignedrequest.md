# PresignedRequest

A presigned request that can be serialized, stored, and executed later.
Hides implementation details for different storage backends.

## Example Usage

```typescript
import { PresignedRequest } from "@alienplatform/manager-api/models";

let value: PresignedRequest = {
  backend: {
    filePath: "/media/before_reporter_thorough.mp2",
    operation: "delete",
    type: "local",
  },
  expiration: new Date("2026-02-01T15:18:44.079Z"),
  operation: "delete",
  path: "/private/var",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `backend`                                                                                     | *models.PresignedRequestBackend*                                                              | :heavy_check_mark:                                                                            | Storage backend representation for different presigned request types                          |
| `expiration`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | When this presigned request expires                                                           |
| `operation`                                                                                   | [models.PresignedOperation](../models/presignedoperation.md)                                  | :heavy_check_mark:                                                                            | The type of operation a presigned request performs                                            |
| `path`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | The path this request operates on                                                             |