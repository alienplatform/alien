# PackageGpgPublicKey

GPG public key for Terraform provider signature verification

## Example Usage

```typescript
import { PackageGpgPublicKey } from "@aliendotdev/platform-api/models";

let value: PackageGpgPublicKey = {
  asciiArmor: "<value>",
  keyId: "<id>",
};
```

## Fields

| Field                    | Type                     | Required                 | Description              |
| ------------------------ | ------------------------ | ------------------------ | ------------------------ |
| `asciiArmor`             | *string*                 | :heavy_check_mark:       | ASCII-armored public key |
| `keyId`                  | *string*                 | :heavy_check_mark:       | GPG key ID               |