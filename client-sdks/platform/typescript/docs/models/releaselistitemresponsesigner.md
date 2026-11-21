# ReleaseListItemResponseSigner

## Example Usage

```typescript
import { ReleaseListItemResponseSigner } from "@alienplatform/platform-api/models";

let value: ReleaseListItemResponseSigner = {
  id: "<id>",
  label: "<value>",
  kind: "recovery",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `id`                                                                           | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `label`                                                                        | *string*                                                                       | :heavy_check_mark:                                                             | Label from the trusted-device list, such as `alice-mbp`                        |
| `kind`                                                                         | [models.ReleaseListItemResponseKind](../models/releaselistitemresponsekind.md) | :heavy_check_mark:                                                             | N/A                                                                            |