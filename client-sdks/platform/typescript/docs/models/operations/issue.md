# Issue

## Example Usage

```typescript
import { Issue } from "@aliendotdev/platform-api/models/operations";

let value: Issue = {
  type: "scheduling_failure",
  message: "<value>",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `type`                                                                                       | [operations.GetContainerAttentionType](../../models/operations/getcontainerattentiontype.md) | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `containerName`                                                                              | *string*                                                                                     | :heavy_minus_sign:                                                                           | N/A                                                                                          |
| `machineId`                                                                                  | *string*                                                                                     | :heavy_minus_sign:                                                                           | N/A                                                                                          |
| `message`                                                                                    | *string*                                                                                     | :heavy_check_mark:                                                                           | N/A                                                                                          |