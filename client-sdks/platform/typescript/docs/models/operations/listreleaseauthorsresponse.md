# ListReleaseAuthorsResponse

Retrieved distinct authors.

## Example Usage

```typescript
import { ListReleaseAuthorsResponse } from "@aliendotdev/platform-api/models/operations";

let value: ListReleaseAuthorsResponse = {
  items: [
    {
      login: null,
      name: "<value>",
      avatarUrl: "https://our-diver.name",
    },
  ],
};
```

## Fields

| Field                                                                       | Type                                                                        | Required                                                                    | Description                                                                 |
| --------------------------------------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| `items`                                                                     | [models.ReleaseAuthorFilterItem](../../models/releaseauthorfilteritem.md)[] | :heavy_check_mark:                                                          | N/A                                                                         |