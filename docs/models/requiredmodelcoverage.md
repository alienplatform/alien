# RequiredModelCoverage

## Example Usage

```typescript
import { RequiredModelCoverage } from "@alienplatform/platform-api/models";

let value: RequiredModelCoverage = {
  publicModelId: "<id>",
  available: false,
  accessStatus: "verified",
  accessObservedAt: new Date("2024-07-16T18:19:22.272Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `publicModelId`                                                                               | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `available`                                                                                   | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `accessStatus`                                                                                | [models.AccessStatus](../models/accessstatus.md)                                              | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `accessObservedAt`                                                                            | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |