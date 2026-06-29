# DefaultReleaseInfoStringList

## Example Usage

```typescript
import { DefaultReleaseInfoStringList } from "@alienplatform/platform-api/models";

let value: DefaultReleaseInfoStringList = {
  type: "stringList",
  value: [
    "<value 1>",
  ],
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `type`                                                                     | [models.ReleaseInfoTypeStringList](../models/releaseinfotypestringlist.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `value`                                                                    | *string*[]                                                                 | :heavy_check_mark:                                                         | String list default.                                                       |