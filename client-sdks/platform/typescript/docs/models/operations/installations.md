# Installations

## Example Usage

```typescript
import { Installations } from "@alienplatform/platform-api/models/operations";

let value: Installations = {
  status: "ready",
  freshness: "unknown",
  sourceUpdatedAt: new Date("2024-10-20T11:47:08.229Z"),
  error: {
    code: "<value>",
    message: "<value>",
  },
  data: null,
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `status`                                                                                      | [operations.InstallationsStatus](../../models/operations/installationsstatus.md)              | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `freshness`                                                                                   | [operations.InstallationsFreshness](../../models/operations/installationsfreshness.md)        | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `sourceUpdatedAt`                                                                             | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `error`                                                                                       | [operations.InstallationsError](../../models/operations/installationserror.md)                | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `data`                                                                                        | [operations.InstallationsData](../../models/operations/installationsdata.md)                  | :heavy_check_mark:                                                                            | N/A                                                                                           |