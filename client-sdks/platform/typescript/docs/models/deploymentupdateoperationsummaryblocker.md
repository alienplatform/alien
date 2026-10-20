# DeploymentUpdateOperationSummaryBlocker

Why the update is blocked. Input values and internal errors are never included.

## Example Usage

```typescript
import { DeploymentUpdateOperationSummaryBlocker } from "@alienplatform/platform-api/models";

let value: DeploymentUpdateOperationSummaryBlocker = {
  kind: "setup",
  message: "<value>",
};
```

## Fields

| Field                                          | Type                                           | Required                                       | Description                                    |
| ---------------------------------------------- | ---------------------------------------------- | ---------------------------------------------- | ---------------------------------------------- |
| `kind`                                         | [models.BlockerKind](../models/blockerkind.md) | :heavy_check_mark:                             | N/A                                            |
| `message`                                      | *string*                                       | :heavy_check_mark:                             | N/A                                            |
| `inputId`                                      | *string*                                       | :heavy_minus_sign:                             | N/A                                            |