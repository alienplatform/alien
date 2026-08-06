# CreateProjectRequirement

## Example Usage

```typescript
import { CreateProjectRequirement } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectRequirement = {
  publicModelId: "<id>",
  clientApis: [],
  required: true,
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `publicModelId`                                                                          | *string*                                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `clientApis`                                                                             | [operations.CreateProjectClientApi](../../models/operations/createprojectclientapi.md)[] | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `required`                                                                               | *boolean*                                                                                | :heavy_check_mark:                                                                       | N/A                                                                                      |