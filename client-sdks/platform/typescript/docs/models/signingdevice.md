# SigningDevice

## Example Usage

```typescript
import { SigningDevice } from "@alienplatform/platform-api/models";

let value: SigningDevice = {
  id: "<id>",
  label: "<value>",
  kind: "ci",
  roles: [
    "<value 1>",
    "<value 2>",
  ],
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `id`                                                       | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `label`                                                    | *string*                                                   | :heavy_check_mark:                                         | Display label, such as `alice-mbp` or `GitHub Actions`     |
| `kind`                                                     | [models.SigningDeviceKind](../models/signingdevicekind.md) | :heavy_check_mark:                                         | N/A                                                        |
| `roles`                                                    | *string*[]                                                 | :heavy_check_mark:                                         | N/A                                                        |