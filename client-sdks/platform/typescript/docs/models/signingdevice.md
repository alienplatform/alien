# SigningDevice

## Example Usage

```typescript
import { SigningDevice } from "@alienplatform/platform-api/models";

let value: SigningDevice = {
  id: "<id>",
  label: "<value>",
  kind: "ci",
  backend: "secure-enclave",
  roles: [],
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `id`                                                             | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `label`                                                          | *string*                                                         | :heavy_check_mark:                                               | Display label, such as `alice-mbp` or `GitHub Actions`           |
| `kind`                                                           | [models.SigningDeviceKind](../models/signingdevicekind.md)       | :heavy_check_mark:                                               | N/A                                                              |
| `backend`                                                        | [models.SigningDeviceBackend](../models/signingdevicebackend.md) | :heavy_check_mark:                                               | Where the key is held, when the signer who added it said         |
| `roles`                                                          | *string*[]                                                       | :heavy_check_mark:                                               | N/A                                                              |