# StackSigning

What setup pins for a protected install.

Take the values from the vendor's install guide or signed release, not from the
control plane.

## Example Usage

```typescript
import { StackSigning } from "@alienplatform/manager-api/models";

let value: StackSigning = {
  channel: "<value>",
  fingerprint: "<value>",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `channel`                                                            | *string*                                                             | :heavy_check_mark:                                                   | Release channel the install follows, such as `production`.           |
| `fingerprint`                                                        | *string*                                                             | :heavy_check_mark:                                                   | `SHA256:<64 hex>` fingerprint of the vendor's signer list version 1. |
