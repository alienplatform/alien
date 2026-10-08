# SetupItemStatusModelRequirement

## Example Usage

```typescript
import { SetupItemStatusModelRequirement } from "@alienplatform/platform-api/models";

let value: SetupItemStatusModelRequirement = {
  publicModelId: "<id>",
  clientApis: [
    "anthropic-messages",
  ],
  required: true,
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `publicModelId`                                                            | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `clientApis`                                                               | [models.SetupItemStatusClientApi](../models/setupitemstatusclientapi.md)[] | :heavy_check_mark:                                                         | N/A                                                                        |
| `required`                                                                 | *boolean*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |