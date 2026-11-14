# ReleaseSigner

One signer of a release.

## Example Usage

```typescript
import { ReleaseSigner } from "@alienplatform/manager-api/models";

let value: ReleaseSigner = {
  id: "<id>",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `id`                                                                       | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `kind`                                                                     | *string*                                                                   | :heavy_minus_sign:                                                         | `device`, `ci` or `recovery`; absent when the signer list doesn't name it. |
| `label`                                                                    | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
