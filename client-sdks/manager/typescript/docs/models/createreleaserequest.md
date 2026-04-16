# CreateReleaseRequest

## Example Usage

```typescript
import { CreateReleaseRequest } from "@alienplatform/manager-api/models";

let value: CreateReleaseRequest = {
  stack: {},
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `gitMetadata`                                                                                  | [models.GitMetadata](../models/gitmetadata.md)                                                 | :heavy_minus_sign:                                                                             | N/A                                                                                            |
| `stack`                                                                                        | [models.StackByPlatform](../models/stackbyplatform.md)                                         | :heavy_check_mark:                                                                             | The release API accepts stacks keyed by platform.<br/>Only one platform stack needs to be present. |