# ConfigureProjectSourceRequirement

## Example Usage

```typescript
import { ConfigureProjectSourceRequirement } from "@alienplatform/platform-api/models/operations";

let value: ConfigureProjectSourceRequirement = {
  publicModelId: "<id>",
  clientApis: [],
  required: false,
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `publicModelId`                                                                                            | *string*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `clientApis`                                                                                               | [operations.ConfigureProjectSourceClientApi](../../models/operations/configureprojectsourceclientapi.md)[] | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `required`                                                                                                 | *boolean*                                                                                                  | :heavy_check_mark:                                                                                         | N/A                                                                                                        |