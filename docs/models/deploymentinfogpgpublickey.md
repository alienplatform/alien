# DeploymentInfoGpgPublicKey

GPG public key for Terraform provider signature verification

## Example Usage

```typescript
import { DeploymentInfoGpgPublicKey } from "@alienplatform/platform-api/models";

let value: DeploymentInfoGpgPublicKey = {
  asciiArmor: "<value>",
  keyId: "<id>",
};
```

## Fields

| Field                    | Type                     | Required                 | Description              |
| ------------------------ | ------------------------ | ------------------------ | ------------------------ |
| `asciiArmor`             | *string*                 | :heavy_check_mark:       | ASCII-armored public key |
| `keyId`                  | *string*                 | :heavy_check_mark:       | GPG key ID               |