# OperatorUpdate

The ready Operator image selected by the project's current configuration when it differs from the image this installation was set up with. Re-applying setup installs it. Null when the installation already uses it, its install identity is unknown, or the selected package is not ready.

## Example Usage

```typescript
import { OperatorUpdate } from "@alienplatform/platform-api/models/operations";

let value: OperatorUpdate = {
  packageId: "<id>",
  packageVersion: "<value>",
  image: "https://loremflickr.com/1957/3352?lock=6533710229739259",
  digest: "<value>",
  builtAt: new Date("2026-02-09T20:38:06.806Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `packageId`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `packageVersion`                                                                              | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `image`                                                                                       | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `digest`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `builtAt`                                                                                     | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
