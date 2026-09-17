# RemoteGcpSandboxBinding

Concrete Agent Platform topology returned to remote clients.

No egress field, unlike the other two clouds: the policy lives on the environment template
named below, so it travels with the template rather than as a flag the client must read.

## Example Usage

```typescript
import { RemoteGcpSandboxBinding } from "@alienplatform/manager-api/models";

let value: RemoteGcpSandboxBinding = {
  engine: "<value>",
  region: "<value>",
  template: "<value>",
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `engine`                                                                                   | *string*                                                                                   | :heavy_check_mark:                                                                         | Reasoning engine the credential lease authorizes sandboxes under.                          |
| `maxLifetimeSeconds`                                                                       | *number*                                                                                   | :heavy_minus_sign:                                                                         | Seconds a sandbox may live, where the declaration asked for one.                           |
| `region`                                                                                   | *string*                                                                                   | :heavy_check_mark:                                                                         | Region selecting the regional aiplatform endpoint.                                         |
| `template`                                                                                 | *string*                                                                                   | :heavy_check_mark:                                                                         | Environment template every sandbox is created from; it carries the image and the ceilings. |