# CreateRemoteSandboxRequest

What a caller chooses. An unknown field is refused, so nothing else can be asked for.

## Example Usage

```typescript
import { CreateRemoteSandboxRequest } from "@alienplatform/manager-api/models";

let value: CreateRemoteSandboxRequest = {
  deploymentId: "<id>",
  resourceId: "<id>",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `deploymentId`                                                                                                       | *string*                                                                                                             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `env`                                                                                                                | Record<string, *string*>                                                                                             | :heavy_minus_sign:                                                                                                   | Variables placed in the sandbox.                                                                                     |
| `labels`                                                                                                             | Record<string, *string*>                                                                                             | :heavy_minus_sign:                                                                                                   | Request-key and expiry labels, the only labels a caller sets. Each is format-checked<br/>here and again by the verifier. |
| `resourceId`                                                                                                         | *string*                                                                                                             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
