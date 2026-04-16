# ReleaseResponse

## Example Usage

```typescript
import { ReleaseResponse } from "@alienplatform/manager-api/models";

let value: ReleaseResponse = {
  createdAt: "1706636647935",
  id: "<id>",
  stack: {},
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `createdAt`                                                                                    | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `gitMetadata`                                                                                  | [models.GitMetadataResponse](../models/gitmetadataresponse.md)                                 | :heavy_minus_sign:                                                                             | N/A                                                                                            |
| `id`                                                                                           | *string*                                                                                       | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `stack`                                                                                        | [models.StackByPlatform](../models/stackbyplatform.md)                                         | :heavy_check_mark:                                                                             | The release API accepts stacks keyed by platform.<br/>Only one platform stack needs to be present. |