# CreateCommandRequest

Request to create a new command

## Example Usage

```typescript
import { CreateCommandRequest } from "@alienplatform/manager-api/models";

let value: CreateCommandRequest = {
  command: "<value>",
  deploymentId: "<id>",
  params: {
    mode: "storage",
  },
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `command`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | Command name (e.g., "generate-report", "sync-data")                                           |
| `deadline`                                                                                    | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | Optional deadline for command completion                                                      |
| `deploymentId`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | Target deployment identifier                                                                  |
| `idempotencyKey`                                                                              | *string*                                                                                      | :heavy_minus_sign:                                                                            | Optional idempotency key                                                                      |
| `params`                                                                                      | *models.BodySpec*                                                                             | :heavy_check_mark:                                                                            | Body specification supporting inline and storage modes                                        |