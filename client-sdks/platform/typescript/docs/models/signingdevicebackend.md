# SigningDeviceBackend

Where the key is held, when the signer who added it said

## Example Usage

```typescript
import { SigningDeviceBackend } from "@alienplatform/platform-api/models";

let value: SigningDeviceBackend = "tpm";
```

## Values

```typescript
"secure-enclave" | "tpm" | "yubikey" | "software"
```