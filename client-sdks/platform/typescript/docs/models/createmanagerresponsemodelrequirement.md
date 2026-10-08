# CreateManagerResponseModelRequirement

## Example Usage

```typescript
import { CreateManagerResponseModelRequirement } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseModelRequirement = {
  publicModelId: "<id>",
  clientApis: [
    "anthropic-messages",
  ],
  required: true,
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `publicModelId`                                                                        | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `clientApis`                                                                           | [models.CreateManagerResponseClientApi](../models/createmanagerresponseclientapi.md)[] | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `required`                                                                             | *boolean*                                                                              | :heavy_check_mark:                                                                     | N/A                                                                                    |