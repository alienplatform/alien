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

| Field                            | Type                             | Required                         | Description                      |
| -------------------------------- | -------------------------------- | -------------------------------- | -------------------------------- |
| `deploymentId`                   | *string*                         | :heavy_check_mark:               | N/A                              |
| `env`                            | Record<string, *string*>         | :heavy_minus_sign:               | Variables placed in the sandbox. |
| `resourceId`                     | *string*                         | :heavy_check_mark:               | N/A                              |
