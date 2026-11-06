# DeploymentSigning

What setup pins for a protected install.

Take the values from the vendor's install guide or signed release, not from the
control plane.

## Example Usage

```typescript
import { DeploymentSigning } from "@alienplatform/platform-api/models";

let value: DeploymentSigning = {
  channel: "<value>",
  fingerprint: "<value>",
};
```

## Fields

| Field                                                                                                                                         | Type                                                                                                                                          | Required                                                                                                                                      | Description                                                                                                                                   |
| --------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| `channel`                                                                                                                                     | *string*                                                                                                                                      | :heavy_check_mark:                                                                                                                            | Release channel the install follows, such as `production`.                                                                                    |
| `fingerprint`                                                                                                                                 | *string*                                                                                                                                      | :heavy_check_mark:                                                                                                                            | `SHA256:<64 hex>` fingerprint of the vendor's signer list version 1.                                                                          |
| `privateInputCustody`                                                                                                                         | *boolean*                                                                                                                                     | :heavy_minus_sign:                                                                                                                            | Resolve private inputs locally even when machine coordination remains hosted.                                                                 |
| `privateMachineApiUrl`                                                                                                                        | *string*                                                                                                                                      | :heavy_minus_sign:                                                                                                                            | Customer setup pins this private instance machine endpoint and enables<br/>install-local input custody. It cannot be changed by the runtime role. |