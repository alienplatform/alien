# ContainerValueRef

## Example Usage

```typescript
import { ContainerValueRef } from "@alienplatform/platform-api/models";

let value: ContainerValueRef = {
  slot: "<value>",
  kind: "plain",
  digest: "<value>",
  updatedAt: new Date("2024-03-26T20:46:37.162Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `slot`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | Slot name of a dynamic container's secret environment variable.                               |
| `kind`                                                                                        | [models.EnvironmentVariableType](../models/environmentvariabletype.md)                        | :heavy_check_mark:                                                                            | Variable type (plain or secret)                                                               |
| `digest`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `updatedAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |